use clap::Parser;
use runst::cli::{Cli, Command};
use runst::history::{DEFAULT_HISTORY_LIMIT, History};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::History {
            count,
            search,
            all,
            json,
            clear,
            path,
        }) => {
            if let Err(e) = handle_history(count, search, all, json, clear, path) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        None => {
            // Default: run the daemon
            if let Err(e) = runst::run() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn handle_history(
    count: usize,
    search: Option<String>,
    all: bool,
    json: bool,
    clear: bool,
    show_path: bool,
) -> runst::error::Result<()> {
    let mut history = History::new(DEFAULT_HISTORY_LIMIT)?;

    if show_path {
        println!("{}", history.path().display());
        return Ok(());
    }

    if clear {
        history.clear()?;
        println!("History cleared.");
        return Ok(());
    }

    let entries = if let Some(ref query) = search {
        history.search(query)
    } else if all {
        history.all()
    } else {
        history.recent(count)
    };

    if entries.is_empty() {
        if search.is_some() {
            println!("No notifications found matching the search query.");
        } else {
            println!("No notifications in history.");
        }
        return Ok(());
    }

    if json {
        let json_output = serde_json::to_string_pretty(&entries)?;
        println!("{}", json_output);
    } else {
        println!(
            "Showing {} notification{}:\n",
            entries.len(),
            if entries.len() == 1 { "" } else { "s" }
        );
        for entry in entries {
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("ID:       {}", entry.id);
            println!("App:      {}", entry.app_name);
            println!("Time:     {}", entry.datetime);
            println!("Urgency:  {}", entry.urgency);
            println!("Summary:  {}", entry.summary);
            if !entry.body.is_empty() {
                println!("Body:     {}", entry.body);
            }
        }
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    }

    Ok(())
}
