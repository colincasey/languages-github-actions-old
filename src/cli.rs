use clap::{Parser, ValueEnum};
use libcnb_data::buildpack::BuildpackId;

#[derive(Parser)]
#[command(bin_name = "actions")]
pub(crate) enum Cli {
    Prepare(PrepareArgs),
    GenerateBuildpackMatrix,
    UpdateBuilder(UpdateBuilderArgs),
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct PrepareArgs {
    #[arg(long, value_enum)]
    pub(crate) bump: BumpCoordinate,
}

#[derive(ValueEnum, Debug, Clone)]
pub(crate) enum BumpCoordinate {
    Major,
    Minor,
    Patch,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct UpdateBuilderArgs {
    #[arg(long)]
    pub(crate) buildpack_id: BuildpackId,
    #[arg(long)]
    pub(crate) buildpack_version: String,
    #[arg(long)]
    pub(crate) buildpack_uri: String,
    #[arg(long, required = true, value_delimiter = ',', num_args = 1..)]
    pub(crate) builders: Vec<String>,
}
