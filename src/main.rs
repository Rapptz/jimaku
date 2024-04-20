use std::{convert::Infallible, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc, time::Duration};

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
use tower::{Layer, Service, ServiceExt as _};
use tower_http::{
    normalize_path::NormalizePathLayer,
    services::{ServeDir, ServeFile},
};
use tracing::{error, info};
use tracing_appender::{non_blocking::WorkerGuard, rolling::Rotation};
use tracing_subscriber::{
    filter::Targets, fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt, Layer as _,
};

fn unwrap_infallible<T>(result: Result<T, Infallible>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => match err {},
    }
}

fn setup_logging() -> anyhow::Result<WorkerGuard> {
    let rust_log_var = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let log_filter = Targets::from_str(&rust_log_var)?;
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(Rotation::DAILY)
        .filename_suffix("log")
        .build(jimaku::utils::logs_directory())?;

    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(non_blocking)
                .with_span_events(FmtSpan::CLOSE)
                .with_filter(log_filter),
        )
        .init();
    Ok(guard)
}

async fn cleanup_old_logs() {
    let (signal_tx, signal_rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        drop(signal_rx);
    });

    let directory = jimaku::utils::logs_directory();
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    loop {
        let now = time::OffsetDateTime::now_utc();
        let today = now.date();
        let cut_off = today - Duration::from_secs(86400 * 60);
        let Ok(dir) = directory.read_dir() else {
            continue;
        };
        for entry in dir {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(prefix) = filename.strip_prefix(".log") else {
                continue;
            };
            let Ok(date) = time::Date::parse(prefix, &fmt) else {
                continue;
            };

            if date < cut_off {
                let _ = tokio::fs::remove_file(path).await;
            }
        }

        let tomorrow = today.next_day().unwrap().midnight().assume_utc();
        let delta = tomorrow - now;
        let next = std::time::Instant::now() + delta;
        tokio::select! {
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(next)) => {

            }
            _ = signal_tx.closed() => {
                info!("Ctrl + C signal received, stopping cleanup loop...");
                return;
            }
        };
    }
}

fn database_directory() -> anyhow::Result<PathBuf> {
    let mut path = dirs::data_dir().context("could not find a data directory for the current user")?;
    path.push(jimaku::PROGRAM_NAME);
    if let Err(e) = std::fs::create_dir(&path) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(e).context("could not create application local data directory");
        }
    }
    path.push("main.db");
    Ok(path)
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

    tokio::spawn(cleanup_old_logs());
    tokio::spawn(jimaku::kitsunekko::auto_scrape_loop(state.clone()));

    // Middleware order for request processing is top to bottom
    // and for response processing it's bottom to top
    let router = jimaku::routes::all()
        .nest_service("/favicon.ico", ServeFile::new("static/icons/favicon.ico"))
        .nest_service("/site.webmanifest", ServeFile::new("static/icons/site.webmanifest"))
        .nest_service("/robots.txt", ServeFile::new("static/robots.txt"))
        .nest_service("/static", ServeDir::new("static"))
        .layer(jimaku::logging::HttpTrace)
        .layer(middleware::from_fn(jimaku::flash::process_flash_messages))
        .layer(middleware::from_fn(jimaku::parse_cookies))
        .layer(Extension(secret_key))
        .layer(Extension(jimaku::cached::BodyCache::new(Duration::from_secs(120))))
        .layer(DefaultBodyLimit::max(jimaku::MAX_BODY_SIZE))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(jimaku::MAX_BODY_SIZE))
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

        let challenge_config = state.challenge_rustls_config();
        let default_config = state.default_rustls_config();
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
            let (tcp, addr) = listener.accept().await.context("Failed during accept loop")?;
            let challenge_config = challenge_config.clone();
            let default_config = default_config.clone();
            let tower_service = unwrap_infallible(service.call(addr).await);

            tokio::spawn(async move {
                let start_handshake = LazyConfigAcceptor::new(Default::default(), tcp).await.unwrap();

                let stream = if is_tls_alpn_challenge(&start_handshake.client_hello()) {
                    info!("Received TLS-ALPN-01 validation request");
                    start_handshake.into_stream(challenge_config).await.unwrap()
                } else {
                    start_handshake.into_stream(default_config).await.unwrap()
                };

                let socket = TokioIo::new(stream);
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

async fn run(command: jimaku::Command) -> anyhow::Result<()> {
    let config = jimaku::Config::load()?;
    let database = jimaku::Database::file(&database_directory()?)
        .with_init(|conn| conn.execute_batch(include_str!("../main.sql")))
        .open()
        .await?;

    let state = jimaku::AppState::new(config, database);
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
            let fixtures: Vec<jimaku::kitsunekko::Fixture> = serde_json::from_str(&buffer)?;
            let total = fixtures.len();
            jimaku::kitsunekko::commit_fixtures(&state, fixtures).await?;
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
