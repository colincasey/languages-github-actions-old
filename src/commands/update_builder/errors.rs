use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub(crate) enum Error {
    GetCurrentDir(std::io::Error),
    InvalidBuildpackUri(String, uriparse::URIReferenceError),
    InvalidBuildpackVersion(String, libcnb_data::buildpack::BuildpackVersionError),
    ReadingBuilder(PathBuf, std::io::Error),
    ParsingBuilder(PathBuf, toml::de::Error),
    WritingBuilder(PathBuf, std::io::Error),
    NoBuilderFiles(Vec<String>),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::GetCurrentDir(error) => {
                write!(f, "Could not get the current directory\nError: {error}")
            }
            Error::InvalidBuildpackUri(value, error) => {
                write!(
                    f,
                    "The buildpack URI argument is invalid\nValue: {value}\nError: {error}"
                )
            }
            Error::InvalidBuildpackVersion(value, error) => {
                write!(
                    f,
                    "The buildpack version argument is invalid\nValue: {value}\nError: {error}"
                )
            }
            Error::ReadingBuilder(path, error) => {
                write!(
                    f,
                    "Error reading builder\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::ParsingBuilder(path, error) => {
                write!(
                    f,
                    "Error parsing builder\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::WritingBuilder(path, error) => {
                write!(
                    f,
                    "Error writing builder\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::NoBuilderFiles(builders) => {
                write!(
                    f,
                    "No builder.toml files found in the given builder directories\n{}",
                    builders
                        .into_iter()
                        .map(|builder| format!("• {builder}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        }
    }
}
