//! A minimal CLI parser with no dependencies.
//!
//! Since our CLI doesn't actually require any flags and is purely
//! subcommand based + positional arguments, it's possible to implement
//! this rather quickly so might as well.

use std::{io::Write, path::PathBuf};

use crate::{auth::hash_password, models::is_valid_username};

pub const PROGRAM_NAME: &str = "jimaku";

/// The subcommand that is currently being executed
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Command {
    Run,
    Admin,
    Scrape { path: Option<PathBuf> },
    Fixtures { path: PathBuf },
    Move { path: PathBuf },
}

macro_rules! quick_exit {
    ($($t:tt)*) => {
        eprintln!($($t)*);
        std::process::exit(1);
    };
}

const HELP_OUTPUT: &str = r#"usage: jimaku <command>

commands:
  run               Runs the server
  admin             Interactively creates an admin user
  scrape   [path]   Scrapes and creates a fixture file from kitsunekko
  fixtures <path>   Loads a fixture from the given path
  move     [path]   Move directory entry paths to a new location

options:
  -h, --help   Prints this help output
"#;

pub struct AdminCredentials {
    pub username: String,
    pub password_hash: String,
}

fn prompt_username() -> std::io::Result<String> {
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let stdin = std::io::stdin();
    let mut buffer = String::new();
    loop {
        stdout.write_all(b"enter username: ")?;
        stdout.flush()?;
        stdin.read_line(&mut buffer)?;
        buffer.truncate(buffer.trim_end().len());

        if is_valid_username(&buffer) {
            return Ok(buffer);
        }
        stderr.write_all(b"username must be all lowercase, numbers or -._ characters")?;
    }
}

fn prompt_password() -> std::io::Result<String> {
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    loop {
        stdout.write_all(b"enter a password: ")?;
        stdout.flush()?;

        let password = rpassword::read_password()?;
        if password.len() > 128 {
            stderr.write_all(b"password too long (must be 8-128 characters)\n")?;
        } else if password.len() < 8 {
            stderr.write_all(b"password too short (must be 8-128 characters)\n")?;
        } else {
            return Ok(password);
        }
    }
}

pub fn prompt_admin_account() -> anyhow::Result<AdminCredentials> {
    let username = prompt_username()?;
    let password = prompt_password()?;
    let password_hash = hash_password(&password)?;
    Ok(AdminCredentials {
        username,
        password_hash,
    })
}

impl Command {
    /// Parses the command line arguments.
    ///
    /// If any error is found then it aborts.
    pub fn parse() -> Self {
        let mut args = std::env::args_os();
        args.next();

        let subcommand = args.next().and_then(|s| s.to_str().map(|s| s.to_lowercase()));
        match subcommand {
            None => Self::Run,
            Some(s) => match s.as_str() {
                "run" => Self::Run,
                "admin" => Self::Admin,
                "scrape" => Self::Scrape {
                    path: args.next().map(PathBuf::from),
                },
                "fixtures" => {
                    let Some(path) = args.next().map(PathBuf::from) else {
                        quick_exit!("missing path parameter");
                    };

                    Self::Fixtures { path }
                }
                "move" => {
                    let Some(path) = args.next().map(PathBuf::from) else {
                        quick_exit!("missing path parameter");
                    };

                    Self::Move { path }
                }
                "-h" | "--help" | "help" => {
                    println!("{HELP_OUTPUT}");
                    std::process::exit(0);
                }
                other => {
                    quick_exit!("unknown subcommand: {other}");
                }
            },
        }
    }
}
