use crate::cli::Cli;
use crate::commands::{generate_buildpack_matrix, prepare, update_builder};
use clap::Parser;

mod cli;
mod commands;
mod github;

const UNSPECIFIED_ERROR: i32 = 1;

fn main() {
    match Cli::parse() {
        Cli::Prepare(args) => {
            if let Err(error) = prepare::execute(args) {
                eprintln!("❌ {error}");
                std::process::exit(UNSPECIFIED_ERROR);
            }
        }

        Cli::GenerateBuildpackMatrix => {
            if let Err(error) = generate_buildpack_matrix::execute() {
                eprintln!("❌ {error}");
                std::process::exit(UNSPECIFIED_ERROR);
            }
        }

        Cli::UpdateBuilder(args) => {
            if let Err(error) = update_builder::execute(args) {
                eprintln!("❌ {error}");
                std::process::exit(UNSPECIFIED_ERROR);
            }
        }
    }
}
