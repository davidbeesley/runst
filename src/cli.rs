//! Command-line interface for runst.

use clap::{Parser, Subcommand};

/// A dead simple notification daemon.
#[derive(Parser, Debug)]
#[command(name = "runst", version, about)]
pub struct Cli {
    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Available subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Query notification history.
    History {
        /// Number of recent notifications to show (default: 10).
        #[arg(short, long, default_value = "10")]
        count: usize,

        /// Search for notifications matching this query.
        #[arg(short, long)]
        search: Option<String>,

        /// Show all notifications (ignores --count).
        #[arg(short, long)]
        all: bool,

        /// Output in JSON format.
        #[arg(short, long)]
        json: bool,

        /// Clear all history.
        #[arg(long)]
        clear: bool,

        /// Show the path to the history file.
        #[arg(long)]
        path: bool,
    },
}
