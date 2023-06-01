use crate::github::actions::SetOutputError;
use libcnb_package::FindBuildpackDirsError;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    GetCurrentDir(std::io::Error),
    FindingBuildpacks(PathBuf, std::io::Error),
    SetActionOutput(std::io::Error),
}

impl From<FindBuildpackDirsError> for Error {
    fn from(value: FindBuildpackDirsError) -> Self {
        match value {
            FindBuildpackDirsError::ReadingMetadata(path, error)
            | FindBuildpackDirsError::ReadingDir(path, error)
            | FindBuildpackDirsError::GetDirEntry(path, error) => {
                Error::FindingBuildpacks(path, error)
            }
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
            Error::FindingBuildpacks(path, error) => {
                write!(
                    f,
                    "I/O error while finding buildpacks\nPath: {}\nError: {error}",
                    path.display()
                )
            }
            Error::SetActionOutput(error) => {
                write!(f, "Could not write action output\nError: {error}")
            }
        }
    }
}
