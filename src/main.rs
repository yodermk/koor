use clap::Parser;
use koor::{
    learning,
    session::Session,
    ui::{self, Frontend, cli::CliFrontend},
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    version,
    about = "Koor: a native trick-taking game for one human and three computers"
)]
struct Options {
    /// Run a headless game with four computer players
    #[arg(long)]
    cli: bool,
    /// Number of completed rounds in CLI mode
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    rounds: u64,
    /// Random seed (combine with --no-learning for reproducible games)
    #[arg(long)]
    seed: Option<u64>,
    /// Disable learning and use the balanced strategy for every computer
    #[arg(long, conflicts_with = "learning_file")]
    no_learning: bool,
    /// Override the persisted strategy learning file
    #[arg(long)]
    learning_file: Option<PathBuf>,
    /// Print every public action in CLI mode
    #[arg(long, requires = "cli", conflicts_with = "quiet")]
    trace: bool,
    /// Print only final totals in CLI mode
    #[arg(long, requires = "cli")]
    quiet: bool,
}

fn main() -> ExitCode {
    let options = Options::parse();
    let seed = options.seed.unwrap_or_else(rand::random);
    let learning_path = if options.no_learning {
        None
    } else {
        Some(options.learning_file.unwrap_or_else(learning::default_path))
    };
    let session = match Session::new(
        seed,
        if options.cli { None } else { Some(0) },
        learning_path,
    ) {
        Ok(session) => session,
        Err(error) => {
            eprintln!(
                "Could not load learning state: {error}. Use --no-learning to play without it."
            );
            return ExitCode::FAILURE;
        }
    };
    if options.cli && !options.quiet {
        println!("Koor simulation · seed {seed} · {} rounds", options.rounds);
    }
    let result = if options.cli {
        CliFrontend {
            rounds: options.rounds,
            trace: options.trace,
            quiet: options.quiet,
        }
        .run(session)
    } else {
        ui::native(session)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
