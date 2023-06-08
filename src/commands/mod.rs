use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use libcnb_data::buildpack::{BuildpackId, BuildpackVersion};
use markdown::mdast::Node;
use markdown::{to_mdast, ParseOptions};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::read_to_string;
use std::ops::Range;
use std::path::{Path, PathBuf};
use toml::Spanned;

pub(crate) mod generate_buildpack_matrix;
pub(crate) mod generate_changelog;
pub(crate) mod prepare_release;
pub(crate) mod update_builder;

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct MinimalBuildpackDescriptor {
    buildpack: BuildpackIdAndVersion,
    #[serde(default)]
    order: Vec<OrderedGroups>,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct OrderedGroups {
    #[serde(default)]
    group: Vec<BuildpackIdAndVersion>,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct BuildpackIdAndVersion {
    id: BuildpackId,
    version: Spanned<BuildpackVersion>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MinimalBuilder {
    buildpacks: Vec<BuilderBuildpack>,
    order: Vec<OrderedGroups>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BuilderBuildpack {
    id: BuildpackId,
    uri: Spanned<String>,
}

pub(crate) struct BuildpackFile {
    path: PathBuf,
    raw: String,
    parsed: MinimalBuildpackDescriptor,
}

#[derive(Debug)]
pub(crate) enum BuildpackFileError {
    Reading(PathBuf, std::io::Error),
    Parsing(PathBuf, toml::de::Error),
}

fn read_buildpack_file_from_dir<P: Into<PathBuf>>(
    dir: P,
) -> Result<BuildpackFile, BuildpackFileError> {
    let path = dir.into().join("buildpack.toml");
    let raw =
        read_to_string(&path).map_err(|error| BuildpackFileError::Reading(path.clone(), error))?;
    let parsed =
        toml::from_str(&raw).map_err(|error| BuildpackFileError::Parsing(path.clone(), error))?;
    Ok(BuildpackFile { path, raw, parsed })
}

pub(crate) struct BuilderFile {
    path: PathBuf,
    raw: String,
    parsed: MinimalBuilder,
}

#[derive(Debug)]
pub(crate) enum BuilderFileError {
    Reading(PathBuf, std::io::Error),
    Parsing(PathBuf, toml::de::Error),
}

fn read_builder_file_from_dir<P: Into<PathBuf>>(dir: P) -> Result<BuilderFile, BuilderFileError> {
    let path = dir.into().join("builder.toml");
    let raw =
        read_to_string(&path).map_err(|error| BuilderFileError::Reading(path.clone(), error))?;
    let parsed =
        toml::from_str(&raw).map_err(|error| BuilderFileError::Parsing(path.clone(), error))?;
    Ok(BuilderFile { path, raw, parsed })
}

pub(crate) struct Changelog {
    unreleased: ChangelogEntrySection,
    versions: HashMap<String, ChangelogEntrySection>,
}

#[derive(Clone)]
pub(crate) struct ChangelogEntrySection {
    span: Range<usize>,
    value: Option<String>,
}

impl Display for ChangelogEntrySection {
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

pub(crate) struct ChangelogEntry {
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

pub(crate) struct ChangelogFile {
    path: PathBuf,
    raw: String,
    parsed: Changelog,
}

#[derive(Debug)]
pub(crate) enum ChangelogFileError {
    Reading(PathBuf, std::io::Error),
    Parsing(PathBuf, String),
}

pub(crate) fn read_changelog_file_from_dir<P: Into<PathBuf>>(
    dir: P,
) -> Result<ChangelogFile, ChangelogFileError> {
    let path = dir.into().join("CHANGELOG.md");
    let raw =
        read_to_string(&path).map_err(|error| ChangelogFileError::Reading(path.clone(), error))?;
    let parsed = parse_changelog(&path, &raw)?;
    Ok(ChangelogFile { path, raw, parsed })
}

fn parse_changelog(path: &Path, changelog_contents: &str) -> Result<Changelog, ChangelogFileError> {
    lazy_static! {
        static ref UNRELEASED_HEADER: Regex =
            Regex::new(r"(?i)^\[unreleased]$").expect("Should be a valid regex");
        static ref VERSION_HEADER: Regex =
            Regex::new(r"^\[(\d+\.\d+\.\d+)]").expect("Should be a valid regex");
    }

    enum SectionType {
        Unreleased,
        Versioned(String),
    }

    let changelog_ast = to_mdast(changelog_contents, &ParseOptions::default())
        .map_err(|error| ChangelogFileError::Parsing(path.to_path_buf(), error))?;

    let unreleased_section_key = "Unreleased";
    let mut current_section_type = None;
    let mut header_nodes_index: HashMap<String, &Node> = HashMap::new();
    let mut body_nodes_index: HashMap<String, &Node> = HashMap::new();

    if let Node::Root(root) = changelog_ast {
        for child in &root.children {
            if let Node::Heading(_) = child {
                let heading = child.to_string();
                if UNRELEASED_HEADER.is_match(heading.as_str()) {
                    current_section_type = Some(SectionType::Unreleased);
                    header_nodes_index.insert(unreleased_section_key.to_string(), child);
                } else if let Some(captures) = VERSION_HEADER.captures(heading.as_str()) {
                    let version = &captures[1];
                    current_section_type = Some(SectionType::Versioned(version.to_string()));
                    header_nodes_index.insert(version.to_string(), child);
                } else {
                    current_section_type = None;
                }
            } else if let Some(section_type) = &current_section_type {
                let section_key = match section_type {
                    SectionType::Unreleased => unreleased_section_key,
                    SectionType::Versioned(version) => version.as_str(),
                };

                if body_nodes_index.contains_key(section_key) {
                    // the only node below the [Unreleased] or [x.y.z] headers should be a single list node
                    // so if this key is already present it means we're setting this with multiple nodes
                    Err(ChangelogFileError::Parsing(
                        path.to_path_buf(),
                        "[{section_key}] contains multiple nodes but only a single node was expected"
                            .to_string(),
                    ))?;
                } else {
                    body_nodes_index.insert(section_key.to_string(), child);
                }
            }
        }

        let mut changelog_entry_sections = HashMap::new();

        for (section_key, header_node) in header_nodes_index {
            let changelog_entry_section = match body_nodes_index.get(&section_key) {
                Some(body_node) => body_node
                    .position()
                    .ok_or(ChangelogFileError::Parsing(
                        path.to_path_buf(),
                        "No position information for changelog entry body".to_string(),
                    ))
                    .map(|position| {
                        let span = position.start.offset..position.end.offset;
                        let value = changelog_contents[span.start..span.end].to_string();
                        ChangelogEntrySection {
                            span,
                            value: Some(value),
                        }
                    })?,
                None => header_node
                    .position()
                    .ok_or(ChangelogFileError::Parsing(
                        path.to_path_buf(),
                        "No position information for changelog entry header".to_string(),
                    ))
                    .map(|position| {
                        // because we're operating with just the header location here but no contents
                        // we need to ensure the range doesn't exceed the actual content length
                        let assumed_end = position.end.offset + 1;
                        let content_end = changelog_contents.len();
                        let span = if assumed_end < content_end {
                            assumed_end..assumed_end
                        } else {
                            content_end..content_end
                        };
                        ChangelogEntrySection { span, value: None }
                    })?,
            };
            changelog_entry_sections.insert(section_key, changelog_entry_section);
        }

        let unreleased_section = changelog_entry_sections
            .remove(unreleased_section_key)
            .ok_or(ChangelogFileError::Parsing(
                path.to_path_buf(),
                "No [Unreleased] header located".to_string(),
            ))?;

        return Ok(Changelog {
            unreleased: unreleased_section,
            versions: changelog_entry_sections,
        });
    }

    Err(ChangelogFileError::Parsing(
        path.to_path_buf(),
        "No root in parsed markdown".to_string(),
    ))
}

#[cfg(test)]
mod test {
    use crate::commands::parse_changelog;
    use std::path::PathBuf;

    #[test]
    fn test_get_unreleased_changes_from_changelog_with_existing_entries() {
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

- 'name' is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))
- Added node version 19.5.0.

"#;

        let path = PathBuf::from("/path/to/CHANGELOG.md");
        let changelog = parse_changelog(&path, changelog).unwrap();
        assert_eq!(
            Some("- Added node version 18.15.0.\n- Added yarn version 4.0.0-rc.2\n".to_string()),
            changelog.unreleased.value
        );
        assert_eq!(
            Some("- Added node version 19.7.0, 19.6.1, 14.21.3, 16.19.1, 18.14.1, 18.14.2.\n- Added node version 18.14.0, 19.6.0.\n".to_string()),
            changelog.versions.get("0.8.16").and_then(|section| section.value.clone())
        );
        assert_eq!(
            Some("- 'name' is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))\n- Added node version 19.5.0.\n".to_string()),
            changelog.versions.get("0.8.15").and_then(|section| section.value.clone())
        );
    }

    #[test]
    fn test_get_unreleased_changes_from_changelog_with_existing_entries_no_spacing() {
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

        let path = PathBuf::from("/path/to/CHANGELOG.md");
        let changelog = parse_changelog(&path, changelog).unwrap();
        assert_eq!(
            Some("- Added node version 18.15.0.\n- Added yarn version 4.0.0-rc.2".to_string()),
            changelog.unreleased.value
        );
    }

    #[test]
    fn test_get_unreleased_changes_from_changelog_with_no_entries() {
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

        let path = PathBuf::from("/path/to/CHANGELOG.md");
        let changelog = parse_changelog(&path, changelog).unwrap();
        assert_eq!(None, changelog.unreleased.value);
    }

    #[test]
    fn test_get_unreleased_changes_from_changelog_initial_state() {
        let changelog = r#"# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
"#;

        let path = PathBuf::from("/path/to/CHANGELOG.md");
        let changelog = parse_changelog(&path, changelog).unwrap();
        assert_eq!(None, changelog.unreleased.value);
    }
}
