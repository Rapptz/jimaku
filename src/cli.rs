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
    Backup { path: PathBuf },
    Upload { path: PathBuf },
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
  move     <path>   Move directory entry paths to a new location
  backup   [path]   Backup subtitles and entry data to the given directory
  upload   <path>   Uploads a backup online

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

                    if path.is_file() {
                        quick_exit!("path must be a directory not a file");
                    }

                    Self::Move { path }
                }
                "backup" => match args.next().map(PathBuf::from) {
                    Some(path) => {
                        if path.is_file() {
                            quick_exit!("path must be a directory not a file");
                        }
                        Self::Backup { path }
                    }
                    None => {
                        let Ok(path) = std::env::current_dir() else {
                            quick_exit!("could not get current directory");
                        };
                        Self::Backup { path }
                    }
                },
                "upload" => {
                    let Some(path) = args.next().map(PathBuf::from) else {
                        quick_exit!("missing path parameter");
                    };

                    if path.is_dir() {
                        quick_exit!("path must be a file not a directory");
                    }

                    Self::Upload { path }
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

// Adapted from https://stackoverflow.com/a/70586588/1381108
pub fn print_progress_bar(step: usize, total: usize, title: Option<&str>) {
    const BLOCKS: [char; 8] = ['█', '▏', '▎', '▍', '▌', '▋', '▊', '█'];
    let mut percentage = 100.0 * (step as f32 / total as f32);
    // n.b.: doesn't work with CJK or emoji characters
    let bar_width = 60 - title.map(|x| x.len() + 2).unwrap_or_default();
    let max_ticks = bar_width * 8;
    let ticks = (percentage / 100.0 * max_ticks as f32).round() as usize;

    let mut to_display = String::with_capacity(256);
    let mut bar = String::with_capacity(bar_width);

    for _ in 0..(ticks / 8) {
        bar.push(BLOCKS[0]);
    }

    let partial_ticks = ticks % 8;
    if partial_ticks > 0 {
        bar.push(BLOCKS[partial_ticks]);
    }

    // Pad progress bar with a fill character
    let fill_ticks = (max_ticks as f32 / 8.0 - ticks as f32 / 8.0) as usize;
    for _ in 0..fill_ticks {
        bar.push('▒');
    }

    if let Some(title) = title {
        to_display.push_str(title);
        to_display.push_str(": ");
    }

    to_display.push_str("\x1b[0;32m"); // Green
    to_display.push_str(&bar);
    to_display.push_str("\x1b[0m"); // Colour reset
    if percentage > 100.0 {
        percentage = 100.0;
    }

    to_display.push_str(&format!(" {step}/{total} {percentage:>5.2}%"));

    eprint!("\r\x1b[2K{to_display}");
}

/// A progress bar that is wrapped over a sequence to show some progress
///
/// It presents it in the following format:
///
/// ```no_rust
/// Title: [Bar goes here] 0/100  0.00%
/// ```
///
/// To append stuff to the end of the progress bar, just use `print!` with a space prefixing it.
/// Whenever a new iteration passes, the line is cleared.
#[derive(Debug)]
pub struct ProgressBar<'a, It> {
    title: Option<&'a str>,
    iter: It,
    total: usize,
    step: usize,
}

impl<'a, It> ProgressBar<'a, It> {
    pub fn new(iter: It, total: usize) -> Self {
        Self {
            title: None,
            total,
            iter,
            step: 0,
        }
    }

    pub fn with_title(self, title: &'a str) -> Self {
        Self {
            title: Some(title),
            iter: self.iter,
            total: self.total,
            step: self.step,
        }
    }
}

/// Clears the progress bar and moves the cursor up
pub fn clear_progress_bar() {
    print!("\r\x1b[2K\x1b[1A");
}

impl<'a, It> Iterator for ProgressBar<'a, It>
where
    It: Iterator,
{
    type Item = It::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.iter.next()?;
        print_progress_bar(self.step, self.total, self.title);
        self.step += 1;
        Some(item)
    }
}
