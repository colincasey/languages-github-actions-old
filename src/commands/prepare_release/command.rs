use crate::commands::prepare_release::errors::Error;
use crate::commands::{
    read_buildpack_file_from_dir, read_changelog_file_from_dir, BuildpackFile, ChangelogEntry,
    ChangelogFile,
};
use crate::github::actions;
use chrono::Utc;
use clap::{Parser, ValueEnum};
use libcnb_data::buildpack::{BuildpackId, BuildpackVersion};
use libcnb_package::{find_buildpack_dirs, FindBuildpackDirsOptions};
use std::collections::{HashMap, HashSet};
use std::fs::write;

type Result<T> = std::result::Result<T, Error>;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct PrepareReleaseArgs {
    #[arg(long, value_enum)]
    pub(crate) bump: BumpCoordinate,
}

#[derive(ValueEnum, Debug, Clone)]
pub(crate) enum BumpCoordinate {
    Major,
    Minor,
    Patch,
}

pub(crate) fn execute(args: PrepareReleaseArgs) -> Result<()> {
    let current_dir = std::env::current_dir().map_err(Error::GetCurrentDir)?;

    let find_buildpack_dirs_options = FindBuildpackDirsOptions {
        ignore: vec![current_dir.join("target")],
    };

    let buildpack_dirs = find_buildpack_dirs(&current_dir, &find_buildpack_dirs_options)
        .map_err(Error::FindingBuildpacks)?;

    if buildpack_dirs.is_empty() {
        Err(Error::NoBuildpacksFound(current_dir))?;
    }

    let buildpack_files = buildpack_dirs
        .iter()
        .map(|dir| read_buildpack_file_from_dir(dir).map_err(Error::BuildpackFile))
        .collect::<Result<Vec<_>>>()?;

    let changelog_files = buildpack_dirs
        .iter()
        .map(|dir| read_changelog_file_from_dir(dir).map_err(Error::ChangelogFile))
        .collect::<Result<Vec<_>>>()?;

    let current_version = get_fixed_version(&buildpack_files)?;

    let next_version = get_next_version(&current_version, args.bump);

    let local_dependencies = buildpack_files
        .iter()
        .map(|buildpack_file| buildpack_file.parsed.buildpack.id.clone())
        .collect::<Vec<_>>();

    for (buildpack_file, changelog_file) in buildpack_files.into_iter().zip(changelog_files) {
        write(
            &buildpack_file.path,
            update_buildpack_contents_with_new_version(
                &buildpack_file,
                &next_version,
                &local_dependencies,
            ),
        )
        .map_err(|e| Error::WritingBuildpack(buildpack_file.path.clone(), e))?;
        eprintln!(
            "✅️ Updated version {current_version} → {next_version}: {}",
            buildpack_file.path.display(),
        );

        let changelog_entry = ChangelogEntry {
            version: next_version.clone(),
            date: Utc::now(),
        };
        write(
            &changelog_file.path,
            update_changelog_with_new_entry(&changelog_file, &changelog_entry),
        )
        .map_err(|e| Error::WritingChangelog(changelog_file.path.clone(), e))?;
        eprintln!(
            "✅️ Added changelog entry \"{changelog_entry}: {}",
            changelog_file.path.display()
        );
    }

    actions::set_output("from_version", current_version.to_string())
        .map_err(Error::SetActionOutput)?;
    actions::set_output("to_version", next_version.to_string()).map_err(Error::SetActionOutput)?;

    Ok(())
}

fn get_fixed_version(buildpack_files: &[BuildpackFile]) -> Result<BuildpackVersion> {
    let version_map = buildpack_files
        .iter()
        .map(|buildpack_file| {
            (
                buildpack_file.path.clone(),
                buildpack_file.parsed.buildpack.version.as_ref().clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let versions = version_map.values().collect::<HashSet<_>>();
    if versions.len() != 1 {
        Err(Error::NotAllVersionsMatch(version_map.clone()))?;
    }
    versions
        .into_iter()
        .next()
        .ok_or(Error::NoFixedVersion)
        .map(|version| version.clone())
}

fn get_next_version(current_version: &BuildpackVersion, bump: BumpCoordinate) -> BuildpackVersion {
    let BuildpackVersion {
        major,
        minor,
        patch,
    } = current_version;

    match bump {
        BumpCoordinate::Major => BuildpackVersion {
            major: major + 1,
            minor: 0,
            patch: 0,
        },
        BumpCoordinate::Minor => BuildpackVersion {
            major: *major,
            minor: minor + 1,
            patch: 0,
        },
        BumpCoordinate::Patch => BuildpackVersion {
            major: *major,
            minor: *minor,
            patch: patch + 1,
        },
    }
}

fn update_buildpack_contents_with_new_version(
    buildpack_file: &BuildpackFile,
    next_version: &BuildpackVersion,
    local_dependencies: &[BuildpackId],
) -> String {
    let contents = &buildpack_file.raw;
    let metadata = &buildpack_file.parsed.buildpack;
    let start = metadata.version.span().start;
    let end = metadata.version.span().end;

    let mut new_contents = format!(
        "{}\"{}\"{}",
        &contents[..start],
        next_version,
        &contents[end..]
    );

    for order_item in &buildpack_file.parsed.order {
        for group_item in &order_item.group {
            if local_dependencies.contains(&group_item.id) {
                new_contents = format!(
                    "{}\"{}\"{}",
                    &new_contents[..group_item.version.span().start],
                    next_version,
                    &new_contents[group_item.version.span().end..]
                );
            }
        }
    }

    new_contents
}

fn update_changelog_with_new_entry(
    changelog_file: &ChangelogFile,
    changelog_entry: &ChangelogEntry,
) -> String {
    let changelog = &changelog_file.parsed;
    let contents = &changelog_file.raw;
    let unreleased = &changelog.unreleased;
    let start = changelog.unreleased.span.start;
    let end = changelog.unreleased.span.end;
    let value = format!(
        "{}\n\n{}\n\n{}\n",
        contents[..start].trim(),
        format!("{changelog_entry}\n\n{unreleased}").trim(),
        contents[end..].trim()
    );
    format!("{}\n", value.trim_end())
}

#[cfg(test)]
mod test {
    use crate::commands::parse_changelog;
    use crate::commands::prepare_release::command::{
        get_fixed_version, update_buildpack_contents_with_new_version,
        update_changelog_with_new_entry, BuildpackFile, ChangelogEntry, ChangelogFile,
    };
    use crate::commands::prepare_release::errors::Error;
    use chrono::{TimeZone, Utc};
    use libcnb_data::buildpack::BuildpackVersion;
    use libcnb_data::buildpack_id;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn test_get_fixed_version() {
        let buildpack_a = create_buildpack_file_with_name(
            "/a/buildpack.toml",
            r#"[buildpack]
id = "a"
version = "0.0.0"
"#,
        );
        let buildpack_b = create_buildpack_file_with_name(
            "/b/buildpack.toml",
            r#"[buildpack]
id = "b"
version = "0.0.0"
"#,
        );
        assert_eq!(
            get_fixed_version(&vec![buildpack_a, buildpack_b]).unwrap(),
            BuildpackVersion {
                major: 0,
                minor: 0,
                patch: 0
            }
        )
    }

    #[test]
    fn test_get_fixed_version_errors_if_there_is_a_version_mismatch() {
        let buildpack_a = create_buildpack_file_with_name(
            "/a/buildpack.toml",
            r#"[buildpack]
id = "a"
version = "0.0.0"
"#,
        );
        let buildpack_b = create_buildpack_file_with_name(
            "/b/buildpack.toml",
            r#"[buildpack]
id = "b"
version = "0.0.1"
"#,
        );
        match get_fixed_version(&vec![buildpack_a, buildpack_b]).unwrap_err() {
            Error::NotAllVersionsMatch(version_map) => {
                assert_eq!(
                    HashMap::from([
                        (
                            PathBuf::from("/a/buildpack.toml"),
                            BuildpackVersion {
                                major: 0,
                                minor: 0,
                                patch: 0
                            }
                        ),
                        (
                            PathBuf::from("/b/buildpack.toml"),
                            BuildpackVersion {
                                major: 0,
                                minor: 0,
                                patch: 1
                            }
                        )
                    ]),
                    version_map
                );
            }
            _ => panic!("Expected error NoFixedVersion"),
        };
    }

    #[test]
    fn test_update_buildpack_contents_with_new_version() {
        let toml = r#"[buildpack]
id = "test"
version = "0.0.0"
            "#;

        let buildpack_file = create_buildpack_file(toml);
        let next_version = BuildpackVersion {
            major: 1,
            minor: 0,
            patch: 0,
        };
        assert_eq!(
            update_buildpack_contents_with_new_version(&buildpack_file, &next_version, &[]),
            r#"[buildpack]
id = "test"
version = "1.0.0"
            "#
        );
    }

    #[test]
    fn test_update_buildpack_contents_with_new_version_and_order_groups_are_present() {
        let toml = r#"[buildpack]
id = "test"
version = "0.0.2"

[[order]]
[[order.group]]
id = "dep-a"
version = "0.0.2"

[[order.group]]
id = "dep-b"
version = "0.0.2"

[[order.group]]
id = "heroku/procfile"
version = "2.0.0"
optional = true
            "#;

        let buildpack_file = create_buildpack_file(toml);
        let next_version = BuildpackVersion {
            major: 1,
            minor: 0,
            patch: 0,
        };
        assert_eq!(
            update_buildpack_contents_with_new_version(
                &buildpack_file,
                &next_version,
                &[buildpack_id!("dep-a"), buildpack_id!("dep-b")]
            ),
            r#"[buildpack]
id = "test"
version = "1.0.0"

[[order]]
[[order.group]]
id = "dep-a"
version = "1.0.0"

[[order.group]]
id = "dep-b"
version = "1.0.0"

[[order.group]]
id = "heroku/procfile"
version = "2.0.0"
optional = true
            "#
        );
    }

    #[test]
    fn test_update_changelog_from_existing_entries() {
        let changelog = r#"# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Added node version 18.15.0.
- Added yarn version 4.0.0-rc.2

## [0.8.16] 2023/02/27

- Added node version 19.7.0, 19.6.1, 14.21.3, 16.19.1, 18.14.1, 18.14.2.
- Added node version 18.14.0, 19.6.0.

## [0.8.15] 2023/02/02

- `name` is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))
- Added node version 19.5.0.
"#;

        let changelog_file = create_changelog_file(changelog);
        let entry = ChangelogEntry {
            version: BuildpackVersion {
                major: 0,
                minor: 8,
                patch: 17,
            },
            date: Utc.with_ymd_and_hms(2023, 5, 29, 0, 0, 0).unwrap(),
        };
        assert_eq!(
            update_changelog_with_new_entry(&changelog_file, &entry),
            r#"# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.17] 2023/05/29

- Added node version 18.15.0.
- Added yarn version 4.0.0-rc.2

## [0.8.16] 2023/02/27

- Added node version 19.7.0, 19.6.1, 14.21.3, 16.19.1, 18.14.1, 18.14.2.
- Added node version 18.14.0, 19.6.0.

## [0.8.15] 2023/02/02

- `name` is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))
- Added node version 19.5.0.
"#
        );
    }

    #[test]
    fn test_update_changelog_from_existing_entries_no_spacing() {
        let changelog = r#"# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
- Added node version 18.15.0.
- Added yarn version 4.0.0-rc.2
## [0.8.16] 2023/02/27

- Added node version 19.7.0, 19.6.1, 14.21.3, 16.19.1, 18.14.1, 18.14.2.
- Added node version 18.14.0, 19.6.0.

## [0.8.15] 2023/02/02

- `name` is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))
- Added node version 19.5.0.
"#;

        let changelog_file = create_changelog_file(changelog);
        let entry = ChangelogEntry {
            version: BuildpackVersion {
                major: 0,
                minor: 8,
                patch: 17,
            },
            date: Utc.with_ymd_and_hms(2023, 5, 29, 0, 0, 0).unwrap(),
        };
        assert_eq!(
            update_changelog_with_new_entry(&changelog_file, &entry),
            r#"# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.17] 2023/05/29

- Added node version 18.15.0.
- Added yarn version 4.0.0-rc.2

## [0.8.16] 2023/02/27

- Added node version 19.7.0, 19.6.1, 14.21.3, 16.19.1, 18.14.1, 18.14.2.
- Added node version 18.14.0, 19.6.0.

## [0.8.15] 2023/02/02

- `name` is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))
- Added node version 19.5.0.
"#
        );
    }

    #[test]
    fn test_update_changelog_from_no_entries() {
        let changelog = r#"# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.16] 2023/02/27

- Added node version 19.7.0, 19.6.1, 14.21.3, 16.19.1, 18.14.1, 18.14.2.
- Added node version 18.14.0, 19.6.0.

## [0.8.15] 2023/02/02

- `name` is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))
- Added node version 19.5.0.
"#;

        let changelog_file = create_changelog_file(changelog);
        let entry = ChangelogEntry {
            version: BuildpackVersion {
                major: 0,
                minor: 8,
                patch: 17,
            },
            date: Utc.with_ymd_and_hms(2023, 5, 29, 0, 0, 0).unwrap(),
        };
        assert_eq!(
            update_changelog_with_new_entry(&changelog_file, &entry),
            r#"# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.17] 2023/05/29

- No Changes

## [0.8.16] 2023/02/27

- Added node version 19.7.0, 19.6.1, 14.21.3, 16.19.1, 18.14.1, 18.14.2.
- Added node version 18.14.0, 19.6.0.

## [0.8.15] 2023/02/02

- `name` is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))
- Added node version 19.5.0.
"#
        );
    }

    #[test]
    fn test_update_changelog_from_initial_state() {
        let changelog = r#"# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
"#;

        let changelog_file = create_changelog_file(changelog);
        let entry = ChangelogEntry {
            version: BuildpackVersion {
                major: 0,
                minor: 0,
                patch: 1,
            },
            date: Utc.with_ymd_and_hms(2023, 5, 29, 0, 0, 0).unwrap(),
        };
        assert_eq!(
            update_changelog_with_new_entry(&changelog_file, &entry),
            r#"# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.1] 2023/05/29

- No Changes
"#
        );
    }

    #[test]
    fn test_update_changelog_from_initial_state_and_no_newline() {
        let changelog = "## [Unreleased]";

        let changelog_file = create_changelog_file(changelog);
        let entry = ChangelogEntry {
            version: BuildpackVersion {
                major: 0,
                minor: 0,
                patch: 1,
            },
            date: Utc.with_ymd_and_hms(2023, 5, 29, 0, 0, 0).unwrap(),
        };
        assert_eq!(
            update_changelog_with_new_entry(&changelog_file, &entry),
            r#"## [Unreleased]

## [0.0.1] 2023/05/29

- No Changes
"#
        );
    }

    fn create_buildpack_file(contents: &str) -> BuildpackFile {
        create_buildpack_file_with_name("/path/to/test/buildpack.toml", contents)
    }

    fn create_buildpack_file_with_name(name: &str, contents: &str) -> BuildpackFile {
        BuildpackFile {
            path: PathBuf::from(name),
            raw: contents.to_string(),
            parsed: toml::from_str(contents).unwrap(),
        }
    }

    fn create_changelog_file(contents: &str) -> ChangelogFile {
        let path = PathBuf::from("/path/to/test/CHANGELOG.md");
        ChangelogFile {
            path: path.clone(),
            raw: contents.to_string(),
            parsed: parse_changelog(&path, contents).unwrap(),
        }
    }
}
