use clap::{Parser, ValueEnum};

#[derive(Parser)]
#[command(bin_name = "actions")]
pub(crate) enum Cli {
    Prepare(PrepareArgs),
    GenerateBuildpackMatrix,
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
