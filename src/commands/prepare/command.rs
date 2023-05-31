use crate::cli::BumpCoordinate;
use crate::commands::prepare::errors::Error;
use crate::github::actions;
use chrono::{DateTime, Utc};
use libcnb_data::buildpack::{BuildpackId, BuildpackVersion};
use libcnb_package::find_buildpack_dirs;
use markdown::mdast::Node;
use markdown::{to_mdast, ParseOptions};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::fs;
use std::fs::write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use toml::Spanned;

pub(crate) fn execute(project_dir: PathBuf, bump: BumpCoordinate) -> Result<()> {
    println!(
        "current dir: {}",
        std::env::current_dir().unwrap().display()
    );
    let buildpack_dirs = find_buildpack_dirs(&project_dir)?;

    if buildpack_dirs.is_empty() {
        Err(Error::NoBuildpacksFound(project_dir.clone()))?;
    }

    let buildpack_files = buildpack_dirs
        .iter()
        .map(|dir| read_buildpack_file(dir))
        .collect::<Result<Vec<_>>>()?;

    let changelog_files = buildpack_dirs
        .iter()
        .map(|dir| read_changelog_file(dir))
        .collect::<Result<Vec<_>>>()?;

    let current_version = get_fixed_version(&buildpack_files)?;

    let next_version = get_next_version(&current_version, bump);

    let changelog_summary =
        get_changelog_summary(buildpack_files.iter().zip(changelog_files.iter()).collect());

    for (buildpack_file, changelog_file) in buildpack_files.into_iter().zip(changelog_files) {
        eprintln!(
            "✅️ Updating version {current_version} → {next_version}: {}",
            buildpack_file.path.display(),
        );
        write(
            &buildpack_file.path,
            update_buildpack_contents_with_new_version(&buildpack_file, &next_version),
        )
        .map_err(|e| Error::WritingBuildpack(buildpack_file.path.clone(), e))?;

        let changelog_entry = ChangelogEntry {
            version: next_version.clone(),
            date: Utc::now(),
        };
        eprintln!(
            "✅️ Adding changelog entry \"{changelog_entry}: {}",
            changelog_file.path.display()
        );
        write(
            &changelog_file.path,
            update_changelog_with_new_entry(&changelog_file, &changelog_entry),
        )
        .map_err(|e| Error::WritingChangelog(changelog_file.path.clone(), e))?;
    }

    actions::set_output("from_version", current_version.to_string())?;
    actions::set_output("to_version", next_version.to_string())?;
    actions::set_output("changelog_summary", changelog_summary)?;

    Ok(())
}

fn read_buildpack_file(dir: &Path) -> Result<BuildpackFile> {
    let path = dir.join("buildpack.toml");
    let raw =
        fs::read_to_string(&path).map_err(|error| Error::ReadingBuildpack(path.clone(), error))?;
    let parsed =
        toml::from_str(&raw).map_err(|error| Error::ParsingBuildpack(path.clone(), error))?;
    Ok(BuildpackFile { path, raw, parsed })
}

fn read_changelog_file(dir: &Path) -> Result<ChangelogFile> {
    let path = dir.join("CHANGELOG.md");
    let raw =
        fs::read_to_string(&path).map_err(|error| Error::ReadingChangelog(path.clone(), error))?;
    let parsed = parse_changelog(&path, &raw)?;
    Ok(ChangelogFile { path, raw, parsed })
}

fn get_changelog_summary(
    buildpacks_with_changelogs: Vec<(&BuildpackFile, &ChangelogFile)>,
) -> String {
    let summary = buildpacks_with_changelogs
        .into_iter()
        .map(|(buildpack_file, changelog_file)| {
            let buildpack_id = buildpack_file.parsed.buildpack.id.to_string();
            let changes = changelog_file.parsed.unreleased.to_string();
            (buildpack_id, changes)
        })
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .map(|(buildpack_id, changes)| format!("## {buildpack_id}\n\n{changes}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{}\n", summary.trim())
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
) -> String {
    let contents = &buildpack_file.raw;
    let metadata = &buildpack_file.parsed.buildpack;
    let start = metadata.version.span().start;
    let end = metadata.version.span().end;
    format!(
        "{}\"{}\"{}",
        &contents[..start],
        next_version,
        &contents[end..]
    )
}

fn parse_changelog(path: &Path, changelog_contents: &str) -> Result<Changelog> {
    let changelog_ast = to_mdast(changelog_contents, &ParseOptions::default())
        .map_err(|error| Error::ParsingChangelog(path.to_path_buf(), error))?;

    let mut in_unreleased_section = false;
    let mut unreleased_header_node = None;
    let mut unreleased_section_nodes = vec![];

    if let Node::Root(root) = changelog_ast {
        let children = root.children;
        for child in &children {
            if let Node::Heading(_) = child {
                let heading = child.to_string();
                if heading.to_lowercase().trim() == "[unreleased]" {
                    in_unreleased_section = true;
                    unreleased_header_node = Some(child);
                } else {
                    in_unreleased_section = false;
                }
            } else if in_unreleased_section {
                unreleased_section_nodes.push(child);
            }
        }

        return if unreleased_section_nodes.is_empty() {
            unreleased_header_node
                .and_then(|node| node.position())
                .ok_or(Error::ParsingChangelog(
                    path.to_path_buf(),
                    "No position information for header".to_string(),
                ))
                .map(|position| {
                    let span = position.end.offset + 1..position.end.offset + 1;
                    Changelog {
                        unreleased: UnreleasedChanges { span, value: None },
                    }
                })
        } else if unreleased_section_nodes.len() == 1 {
            unreleased_section_nodes
                .into_iter()
                .next()
                .and_then(|node| node.position())
                .ok_or(Error::ParsingChangelog(
                    path.to_path_buf(),
                    "No position information for unreleased section".to_string(),
                ))
                .map(|position| {
                    let span = position.start.offset..position.end.offset;
                    let value = changelog_contents[span.start..span.end].to_string();
                    Changelog {
                        unreleased: UnreleasedChanges {
                            span,
                            value: Some(value),
                        },
                    }
                })
        } else {
            // something is off, the only node below the unreleased changes section should be a single list node
            Err(Error::ParsingChangelog(
                path.to_path_buf(),
                "Unreleased section contains multiple nodes but only a single was expected"
                    .to_string(),
            ))
        };
    }

    Err(Error::ParsingChangelog(
        path.to_path_buf(),
        "No root in parsed markdown".to_string(),
    ))
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
        "{}\n\n{}\n\n{}",
        &contents[..start].trim_end(),
        format!("{changelog_entry}\n\n{unreleased}").trim(),
        &contents[end..].trim_start()
    );
    format!("{}\n", value.trim_end())
}

type Result<T> = std::result::Result<T, Error>;

struct BuildpackFile {
    path: PathBuf,
    raw: String,
    parsed: MinimalBuildpackDescriptor,
}

struct ChangelogFile {
    path: PathBuf,
    raw: String,
    parsed: Changelog,
}

struct Changelog {
    unreleased: UnreleasedChanges,
}

struct UnreleasedChanges {
    span: Range<usize>,
    value: Option<String>,
}

impl Display for UnreleasedChanges {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self.value {
                Some(changes) => changes,
                None => "- No Changes",
            }
        )
    }
}

#[derive(Deserialize, Debug, Clone)]
struct MinimalBuildpackDescriptor {
    buildpack: BuildpackMetadata,
}

#[derive(Deserialize, Debug, Clone)]
struct BuildpackMetadata {
    id: BuildpackId,
    version: Spanned<BuildpackVersion>,
}

struct ChangelogEntry {
    version: BuildpackVersion,
    date: DateTime<Utc>,
}

impl Display for ChangelogEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "## [{}] {}",
            &self.version,
            &self.date.format("%Y/%m/%d")
        )
    }
}

#[cfg(test)]
mod test {
    use crate::commands::prepare::command::{
        get_changelog_summary, get_fixed_version, parse_changelog,
        update_buildpack_contents_with_new_version, update_changelog_with_new_entry, BuildpackFile,
        ChangelogEntry, ChangelogFile,
    };
    use crate::commands::prepare::errors::Error;
    use chrono::{TimeZone, Utc};
    use libcnb_data::buildpack::BuildpackVersion;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn test_changelog_summary() {
        let buildpack_a = create_buildpack_file(
            r#"
[buildpack]
id = "a"
version = "0.0.0"
"#,
        );
        let changelog_a = create_changelog_file(
            r#"
# Changelog

## [Unreleased]

- change from a
"#,
        );
        let buildpack_b = create_buildpack_file(
            r#"
[buildpack]
id = "b"
version = "0.0.0"
"#,
        );
        let changelog_b = create_changelog_file(
            r#"
# Changelog

## [Unreleased]

- change from b
- change from b
"#,
        );

        let buildpacks_with_changelogs =
            vec![(&buildpack_b, &changelog_b), (&buildpack_a, &changelog_a)];

        assert_eq!(
            get_changelog_summary(buildpacks_with_changelogs),
            r#"## a

- change from a

## b

- change from b
- change from b
"#
        )
    }

    #[test]
    fn test_get_fixed_version() {
        let buildpack_a = create_buildpack_file_with_name(
            "/a/buildpack.toml",
            r#"
[buildpack]
id = "a"
version = "0.0.0"
"#,
        );
        let buildpack_b = create_buildpack_file_with_name(
            "/b/buildpack.toml",
            r#"
[buildpack]
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
            r#"
[buildpack]
id = "a"
version = "0.0.0"
"#,
        );
        let buildpack_b = create_buildpack_file_with_name(
            "/b/buildpack.toml",
            r#"
[buildpack]
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
        let toml = r#"
[buildpack]
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
            update_buildpack_contents_with_new_version(&buildpack_file, &next_version),
            r#"
[buildpack]
id = "test"
version = "1.0.0"
            "#
        );
    }

    #[test]
    fn test_get_unreleased_changes_from_changelog_with_existing_entries() {
        let changelog = r#"
# Changelog
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
        assert_eq!(
            Some("- Added node version 18.15.0.\n- Added yarn version 4.0.0-rc.2\n".to_string()),
            changelog_file.parsed.unreleased.value
        );
    }

    #[test]
    fn test_update_changelog_from_existing_entries() {
        let changelog = r#"
# Changelog
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
            r#"
# Changelog
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
    fn test_get_unreleased_changes_from_changelog_with_existing_entries_no_spacing() {
        let changelog = r#"
# Changelog
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
        assert_eq!(
            Some("- Added node version 18.15.0.\n- Added yarn version 4.0.0-rc.2".to_string()),
            changelog_file.parsed.unreleased.value
        );
    }

    #[test]
    fn test_update_changelog_from_existing_entries_no_spacing() {
        let changelog = r#"
# Changelog
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
            r#"
# Changelog
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
    fn test_get_unreleased_changes_from_changelog_with_no_entries() {
        let changelog = r#"
# Changelog
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
        assert_eq!(None, changelog_file.parsed.unreleased.value);
    }

    #[test]
    fn test_update_changelog_from_no_entries() {
        let changelog = r#"
# Changelog
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
            r#"
# Changelog
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
    fn test_get_unreleased_changes_from_changelog_initial_state() {
        let changelog = r#"
# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
            "#;

        let changelog_file = create_changelog_file(changelog);
        assert_eq!(None, changelog_file.parsed.unreleased.value);
    }

    #[test]
    fn test_update_changelog_from_initial_state() {
        let changelog = r#"
# Changelog
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
            r#"
# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
