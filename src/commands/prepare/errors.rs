use crate::github::actions::SetOutputError;
use libcnb_data::buildpack::BuildpackVersion;
use libcnb_package::FindBuildpackDirsError;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    GetCurrentDir(io::Error),
    NoBuildpacksFound(PathBuf),
    NotAllVersionsMatch(HashMap<PathBuf, BuildpackVersion>),
    NoFixedVersion,
    FindingBuildpacks(PathBuf, io::Error),
    ReadingBuildpack(PathBuf, io::Error),
    ParsingBuildpack(PathBuf, toml::de::Error),
    WritingBuildpack(PathBuf, io::Error),
    ReadingChangelog(PathBuf, io::Error),
    ParsingChangelog(PathBuf, String),
    WritingChangelog(PathBuf, io::Error),
    SetActionOutput(io::Error),
}

impl From<FindBuildpackDirsError> for Error {
    fn from(value: FindBuildpackDirsError) -> Self {
        match value {
            FindBuildpackDirsError::IO(path, error) => Error::FindingBuildpacks(path, error),
        }
    }
}

impl From<SetOutputError> for Error {
    fn from(value: SetOutputError) -> Self {
        match value {
            SetOutputError::Opening(error) | SetOutputError::Writing(error) => {
                Error::SetActionOutput(error)
            }
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::GetCurrentDir(error) => {
                write!(f, "Failed to get current directory\nError: {error}")
            }
            Error::NoBuildpacksFound(path) => {
                write!(f, "No buildpacks found under {}", path.display())
            }
            Error::NotAllVersionsMatch(version_map) => {
                write!(
                    f,
                    "Not all versions match:\n{}",
                    version_map
                        .iter()
                        .map(|(path, version)| format!("• {version} ({})", path.display()))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
            Error::NoFixedVersion => {
                write!(f, "No fixed version could be determined")
            }
            Error::FindingBuildpacks(path, error) => {
                write!(
                    f,
                    "I/O error while finding buildpacks\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::ReadingBuildpack(path, error) => {
                write!(
                    f,
                    "Could not read buildpack\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::ParsingBuildpack(path, error) => {
                write!(
                    f,
                    "Could not parse buildpack\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::WritingBuildpack(path, error) => {
                write!(
                    f,
                    "Could not write buildpack\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::ReadingChangelog(path, error) => {
                write!(
                    f,
                    "Could not read changelog\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::ParsingChangelog(path, error) => {
                write!(
                    f,
                    "Could not parse changelog\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::WritingChangelog(path, error) => {
                write!(
                    f,
                    "Could not write changelog\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::SetActionOutput(error) => {
                write!(f, "Could not write action output\nError: {error}")
            }
        }
    }
}
