use crate::commands::{BuildpackFileError, ChangelogFileError};
use crate::github::actions::SetOutputError;
use libcnb_data::buildpack::BuildpackVersion;
use libcnb_package::FindBuildpackDirsError;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum Error {
    GetCurrentDir(io::Error),
    NoBuildpacksFound(PathBuf),
    NotAllVersionsMatch(HashMap<PathBuf, BuildpackVersion>),
    NoFixedVersion,
    FindingBuildpacks(FindBuildpackDirsError),
    BuildpackFile(BuildpackFileError),
    ChangelogFile(ChangelogFileError),
    WritingBuildpack(PathBuf, io::Error),
    WritingChangelog(PathBuf, io::Error),
    SetActionOutput(SetOutputError),
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

            Error::FindingBuildpacks(finding_buildpack_dirs_error) => {
                match finding_buildpack_dirs_error {
                    FindBuildpackDirsError::IO(path, error) => {
                        write!(
                            f,
                            "I/O error while finding buildpacks\nPath: {}\nError: {error}",
                            path.display()
                        )
                    }
                }
            }

            Error::BuildpackFile(buildpack_file_error) => match buildpack_file_error {
                BuildpackFileError::Reading(path, error) => {
                    write!(
                        f,
                        "Could not read buildpack\nPath: {}\nError: {error}",
                        path.display()
                    )
                }

                BuildpackFileError::Parsing(path, error) => {
                    write!(
                        f,
                        "Could not parse buildpack\nPath: {}\nError: {error}",
                        path.display()
                    )
                }
            },

            Error::WritingBuildpack(path, error) => {
                write!(
                    f,
                    "Could not write buildpack\nPath: {}\nError: {error}",
                    path.display()
                )
            }

            Error::ChangelogFile(changelog_file_error) => match changelog_file_error {
                ChangelogFileError::Reading(path, error) => {
                    write!(
                        f,
                        "Could not read changelog\nPath: {}\nError: {error}",
                        path.display()
                    )
                }

                ChangelogFileError::Parsing(path, error) => {
                    write!(
                        f,
                        "Could not parse changelog\nPath: {}\nError: {error}",
                        path.display()
                    )
                }
            },

            Error::WritingChangelog(path, error) => {
                write!(
                    f,
                    "Could not write changelog\nPath: {}\nError: {error}",
                    path.display()
                )
            }

            Error::SetActionOutput(set_output_error) => match set_output_error {
                SetOutputError::Opening(error) | SetOutputError::Writing(error) => {
                    write!(f, "Could not write action output\nError: {error}")
                }
            },
        }
    }
}
