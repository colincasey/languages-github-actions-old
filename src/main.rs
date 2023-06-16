use crate::commands::generate_changelog::command::GenerateChangelogArgs;
use crate::commands::prepare_release::command::PrepareReleaseArgs;
use crate::commands::update_builder::command::UpdateBuilderArgs;
use crate::commands::{
    generate_buildpack_matrix, generate_changelog, prepare_release, update_builder,
};
use clap::Parser;

mod changelog;
mod commands;
mod github;

const UNSPECIFIED_ERROR: i32 = 1;

#[derive(Parser)]
#[command(bin_name = "actions")]
pub(crate) enum Cli {
    GenerateBuildpackMatrix,
    GenerateChangelog(GenerateChangelogArgs),
    PrepareRelease(PrepareReleaseArgs),
    UpdateBuilder(UpdateBuilderArgs),
}

fn main() {
    match Cli::parse() {
        Cli::GenerateBuildpackMatrix => {
            if let Err(error) = generate_buildpack_matrix::execute() {
                eprintln!("❌ {error}");
                std::process::exit(UNSPECIFIED_ERROR);
            }
        }

        Cli::GenerateChangelog(args) => {
            if let Err(error) = generate_changelog::execute(args) {
                eprintln!("❌ {error}");
                std::process::exit(UNSPECIFIED_ERROR);
            }
        }

        Cli::PrepareRelease(args) => {
            if let Err(error) = prepare_release::execute(args) {
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
