use crate::cli::Cli;
use crate::commands::prepare::execute;
use clap::Parser;
use std::path::PathBuf;

mod cli;
mod commands;
mod github;

const UNSPECIFIED_ERROR: i32 = 1;

fn main() {
    match Cli::parse() {
        Cli::Prepare(args) => {
            let project_dir = PathBuf::from(args.project_dir);
            if let Err(error) = execute(project_dir, args.bump) {
                eprintln!("❌ {error}");
                std::process::exit(UNSPECIFIED_ERROR);
            }
        }
    }
}
