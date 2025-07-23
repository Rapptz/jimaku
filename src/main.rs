use std::{convert::Infallible, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};

use anyhow::Context;
use axum::{
    extract::{DefaultBodyLimit, Request},
    middleware, Extension, ServiceExt,
};
use futures_util::StreamExt;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls_acme::AcmeConfig;
use rustls_acme::{caches::DirCache, is_tls_alpn_challenge};
use tokio_rustls::LazyConfigAcceptor;
use tower::{limit::GlobalConcurrencyLimitLayer, Layer, Service, ServiceExt as _};
use tower_http::{
    compression::CompressionLayer,
    normalize_path::NormalizePathLayer,
    services::{ServeDir, ServeFile},
    timeout::TimeoutLayer,
};
use tracing::{error, info};
use tracing_appender::{non_blocking::WorkerGuard, rolling::Rotation};
use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    fmt::format::FmtSpan,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    Layer as _,
};

fn unwrap_infallible<T>(result: Result<T, Infallible>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => match err {},
    }
}

fn setup_logging() -> anyhow::Result<(WorkerGuard, WorkerGuard)> {
    let rust_log_var = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let log_filter = Targets::from_str(&rust_log_var)?.with_target("bad_request", LevelFilter::OFF);
    let file_appender = tracing_appender::rolling::Builder::new()
        .max_log_files(60)
        .symlink("today.log")
        .rotation(Rotation::DAILY)
        .filename_suffix("log")
        .build(jimaku::utils::logs_directory())?;

    let bad_request_filter = Targets::new().with_target("bad_request", tracing::Level::INFO);
    let bad_request = tracing_appender::rolling::Builder::new()
        .max_log_files(5)
        .symlink("bad_requests.log")
        .rotation(Rotation::DAILY)
        .filename_prefix("bad_requests")
        .filename_suffix("log")
        .build(jimaku::utils::logs_directory())?;

    let (non_blocking_main, main_guard) = tracing_appender::non_blocking(file_appender);
    let (non_blocking_bad_req, bad_req_guard) = tracing_appender::non_blocking(bad_request);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(non_blocking_main)
                .with_span_events(FmtSpan::CLOSE)
                .with_filter(log_filter),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .compact()
                .with_ansi(false)
                .with_level(false)
                .with_target(false)
                .with_writer(non_blocking_bad_req)
                .with_filter(bad_request_filter),
        )
        .init();
    Ok((main_guard, bad_req_guard))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn run_server(state: jimaku::AppState) -> anyhow::Result<()> {
    let config = state.config().clone();
    let _ = jimaku::CONFIG.set(config.clone());
    let addr = config.server.address();
    let secret_key = config.secret_key;

    let request_logger = state.requests.clone();
    let notifications = state.notifications.clone();
    tokio::spawn(jimaku::kitsunekko::auto_scrape_loop(state.clone()));
    tokio::spawn(jimaku::jpsubbers::auto_scrape_loop(state.clone()));
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            notifications.cleanup();
            if !request_logger.cleanup() {
                break;
            }
        }
    });

    // Middleware order for request processing is bottom to top
    // and for response processing it's top to bottom
    let router = jimaku::routes::all()
        .nest_service("/favicon.ico", ServeFile::new("static/icons/favicon.ico"))
        .nest_service("/site.webmanifest", ServeFile::new("static/icons/site.webmanifest"))
        .nest_service("/robots.txt", ServeFile::new("static/robots.txt"))
        .nest_service("/static", ServeDir::new("static"))
        .layer(middleware::from_fn_with_state(state.clone(), jimaku::copy_api_token))
        .layer(jimaku::logging::HttpTrace::new(state.requests.clone()))
        .layer(middleware::from_fn(jimaku::flash::process_flash_messages))
        .layer(middleware::from_fn(jimaku::parse_cookies))
        .layer(Extension(secret_key))
        .layer(Extension(jimaku::cached::BodyCache::new(Duration::from_secs(120))))
        .layer(DefaultBodyLimit::max(jimaku::MAX_BODY_SIZE))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(jimaku::MAX_BODY_SIZE))
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .layer(GlobalConcurrencyLimitLayer::new(512))
        .with_state(state);

    let app = NormalizePathLayer::trim_trailing_slash().layer(router);
    let mut service = ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(app);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Could not bind to {addr}"))?;

    if !config.production || addr.port() != 443 {
        axum::serve(listener, service)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .context("Failed during server service")?;

        return Ok(());
    }

    // Production server stuff
    if addr.port() == 443 {
        let cache_dir = dirs::cache_dir()
            .map(|p| p.join(jimaku::PROGRAM_NAME).join("rustls_acme_cache"))
            .context("Could not find appropriate cache location for ACME")?;
        let mut state = AcmeConfig::new(config.domains)
            .contact(config.contact_emails.iter().map(|x| format!("mailto:{x}")))
            .cache(DirCache::new(cache_dir))
            .directory_lets_encrypt(config.lets_encrypt_production)
            .state();

        let supported_alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let mut challenge_config = state.challenge_rustls_config();
        let mut default_config = state.default_rustls_config();
        if let Some(config) = Arc::get_mut(&mut challenge_config) {
            config.alpn_protocols.extend(supported_alpn_protocols.clone());
        }
        if let Some(config) = Arc::get_mut(&mut default_config) {
            config.alpn_protocols.extend(supported_alpn_protocols);
        }
        tokio::spawn(async move {
            loop {
                match state.next().await {
                    Some(Ok(ok)) => info!("ACME event: {:?}", ok),
                    Some(Err(err)) => error!("ACME error: {:?}", err),
                    None => break,
                }
            }
        });

        loop {
            let (tcp, addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    // Connection errors can be ignored
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::ConnectionReset
                    ) {
                        continue;
                    }

                    // If we get any other type of error then just log it and sleep for a little bit
                    // and try again. According to hyper's old server implementation
                    // https://github.com/hyperium/hyper/blob/v0.14.27/src/server/tcp.rs#L184-L198
                    //
                    // They used to sleep if the file limit was reached, presumably to let other files
                    // close down.
                    error!("error during accept loop: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            let sock_ref = socket2::SockRef::from(&tcp);
            let keep_alive = socket2::TcpKeepalive::new()
                .with_time(Duration::from_secs(60))
                .with_interval(Duration::from_secs(10));
            let _ = sock_ref.set_tcp_keepalive(&keep_alive);
            let challenge_config = challenge_config.clone();
            let default_config = default_config.clone();
            let tower_service = unwrap_infallible(service.call(addr).await);

            tokio::spawn(async move {
                let start_handshake = match LazyConfigAcceptor::new(Default::default(), tcp).await {
                    Err(e) => {
                        eprintln!("failed to start handshake accept: {e:?}");
                        return;
                    }
                    Ok(s) => s,
                };

                let stream = if is_tls_alpn_challenge(&start_handshake.client_hello()) {
                    info!("Received TLS-ALPN-01 validation request");
                    start_handshake.into_stream(challenge_config).await
                } else {
                    start_handshake.into_stream(default_config).await
                };

                let socket = match stream {
                    Err(e) => {
                        eprintln!("failed to start handshake: {e:?}");
                        return;
                    }
                    Ok(stream) => TokioIo::new(stream),
                };
                let hyper_service = hyper::service::service_fn(move |request: Request<Incoming>| {
                    tower_service.clone().oneshot(request)
                });

                let serve = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(socket, hyper_service)
                    .await;
                if let Err(e) = serve {
                    eprintln!("failed to serve connection: {e:#}");
                }
            });
        }
    }

    axum::serve(listener, service)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Failed during server service")?;
    Ok(())
}

const MIGRATIONS: [&str; 3] = [
    include_str!("../sql/0.sql"),
    include_str!("../sql/1.sql"),
    include_str!("../sql/2.sql"),
];

fn init_db(connection: &mut rusqlite::Connection) -> rusqlite::Result<()> {
    connection.execute_batch("PRAGMA foreign_keys=1;\nPRAGMA journal_mode=wal;")?;
    let tx = connection.transaction()?;
    let version: usize = {
        let mut stmt = tx.prepare_cached("PRAGMA user_version;")?;
        stmt.query_row([], |r| r.get(0))?
    };
    for migration in MIGRATIONS.iter().skip(version) {
        tx.execute_batch(migration)?;
    }
    tx.commit()
}

async fn run(command: jimaku::Command) -> anyhow::Result<()> {
    let config = jimaku::Config::load()?;
    let database = jimaku::Database::file(&jimaku::database::directory()?)
        .with_init(init_db)
        .open()
        .await?;

    let state = jimaku::AppState::new(config, database).await;
    match command {
        jimaku::Command::Run => run_server(state).await,
        jimaku::Command::Admin => {
            let credentials = jimaku::cli::prompt_admin_account()?;
            let mut flags = jimaku::models::AccountFlags::default();
            flags.set_admin(true);
            state
                .database()
                .execute(
                    "INSERT INTO account(name, password, flags) VALUES (?, ?, ?)",
                    (credentials.username.clone(), credentials.password_hash, flags),
                )
                .await?;
            info!("successfully created account {}", credentials.username);
            Ok(())
        }
        jimaku::Command::Scrape { path } => {
            let date = state
                .database()
                .get_from_storage::<time::OffsetDateTime>("kitsunekko_scrape_date")
                .await
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);

            info!("scraping kitsunekko entries newer than {}", &date);
            let fixtures = jimaku::kitsunekko::scrape(&state, date).await?;
            let path = path.unwrap_or("fixtures.json".into());
            let fp = std::fs::File::create(path)?;
            serde_json::to_writer(fp, &fixtures)?;
            if let Some(date) = fixtures.iter().map(|x| x.last_updated_at).max() {
                state.database().update_storage("kitsunekko_scrape_date", date).await?;
            }
            Ok(())
        }
        jimaku::Command::Fixtures { path } => {
            let buffer = std::fs::read_to_string(path)?;
            let fixtures: Vec<jimaku::fixture::Fixture> = serde_json::from_str(&buffer)?;
            let total = fixtures.len();
            jimaku::fixture::commit_fixtures(&state, fixtures).await?;
            info!("committed {} fixtures to the database", total);
            Ok(())
        }
        jimaku::Command::Move { path } => {
            // First get all the directory entries
            let mut entries: Vec<jimaku::models::DirectoryEntry> =
                state.database().all("SELECT * FROM directory_entry", []).await?;
            let mut skipped = 0;
            let total = entries.len();
            // Clippy bug
            #[allow(clippy::explicit_counter_loop)]
            for entry in entries.iter_mut() {
                let Ok(suffix) = entry.path.strip_prefix(&state.config().subtitle_path) else {
                    skipped += 1;
                    continue;
                };
                entry.path = path.join(suffix);
            }
            let query = "UPDATE directory_entry SET path = ? WHERE id = ?";
            state
                .database()
                .call(move |conn| -> rusqlite::Result<()> {
                    let tx = conn.transaction()?;
                    {
                        let mut stmt = tx.prepare(query)?;
                        for entry in entries {
                            stmt.execute((entry.path.to_string_lossy(), entry.id))?;
                        }
                    }
                    tx.commit()?;
                    Ok(())
                })
                .await?;

            info!(
                "Successfully moved {} entries ({} were skipped)",
                total - skipped,
                skipped
            );
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() {
    let _guard = match setup_logging() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("Error setting up logger:\n{e:?}");
            return;
        }
    };

    let command = jimaku::Command::parse();
    if let Err(e) = run(command).await {
        eprintln!("Error occurred during main execution:\n{e:?}");

        error!(error = %e,"error occurred during main execution");
        for e in e.chain().skip(1) {
            error!(cause = %e)
        }
    }
}
