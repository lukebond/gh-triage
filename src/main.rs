mod config;
mod daemon;
mod db;
mod github;
mod notify;
mod setup;
mod summary;
mod tui;
mod types;
mod waybar;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(#[from] config::ConfigError),
    #[error("database error: {0}")]
    Db(#[from] db::DbError),
    #[error("github error: {0}")]
    Github(#[from] github::GithubError),
    #[error("TUI error: {0}")]
    Tui(String),
    #[error("setup error: {0}")]
    Setup(String),
}

#[derive(Parser)]
#[command(name = "gh-triage", about = "GitHub notification triage tool")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run polling daemon
    Daemon,
    /// Print Waybar JSON and exit
    Waybar,
    /// Run one poll cycle and exit
    Poll,
    /// Print active items to stdout
    List,
    /// Archive an item by ID
    Archive {
        /// The item node_id to archive
        id: String,
    },
    /// Launch TUI (same as running with no subcommand)
    Tui,
    /// Setup helpers for systemd and Waybar
    #[command(subcommand)]
    Setup(SetupCommands),
}

#[derive(Subcommand)]
enum SetupCommands {
    /// Install systemd user service for the daemon
    Systemd,
    /// Print Waybar config snippet
    Waybar {
        /// Terminal emulator to use for on-click (default: foot)
        #[arg(long, short)]
        terminal: Option<String>,
    },
}

fn db_path() -> PathBuf {
    config::data_dir().join("db.sqlite")
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), AppError> {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Commands::Tui) => {
            let config = config::Config::load()?;
            tui::run_tui(&config, &db_path())?;
        }
        Some(Commands::Daemon) => {
            let config = config::Config::load()?;
            daemon::run_daemon(config, &db_path()).await?;
        }
        Some(Commands::Waybar) => {
            let config = config::Config::load()?;
            drop(config); // only needed for validation
            waybar::run_waybar(&db_path())?;
        }
        Some(Commands::Poll) => {
            let config = config::Config::load()?;
            daemon::run_poll(&config, &db_path()).await?;
        }
        Some(Commands::List) => {
            let _config = config::Config::load()?;
            let db = db::Db::open(&db_path())?;
            let items = db.get_items(types::ItemStatus::Active)?;
            if items.is_empty() {
                println!("No active items.");
            } else {
                for item in &items {
                    let type_label = item.item_type.short_label();
                    let repo_short = item.repo.split('/').nth(1).unwrap_or(&item.repo);
                    println!(
                        "{} {} {:<16} {}",
                        type_label, item.state, repo_short, item.title
                    );
                    if let Some(ref summary) = item.summary {
                        println!("  {summary}");
                    }
                }
            }
        }
        Some(Commands::Archive { id }) => {
            let _config = config::Config::load()?;
            let db = db::Db::open(&db_path())?;
            if db.archive_item(&id)? {
                println!("Archived {id}");
            } else {
                println!("Item not found or already archived: {id}");
            }
        }
        Some(Commands::Setup(sub)) => match sub {
            SetupCommands::Systemd => setup::install_systemd()?,
            SetupCommands::Waybar { terminal } => setup::install_waybar(terminal)?,
        },
    }

    Ok(())
}
