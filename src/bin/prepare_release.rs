use chrono::Utc;
use clap::{Parser, ValueEnum};
use console::Emoji;
use glob::glob;
use libcnb_data::buildpack::{BuildpackDescriptor, BuildpackId, BuildpackVersion};
use std::collections::{HashMap, HashSet};
use std::fs::write;
use std::path::{Path, PathBuf};
use std::{env, fs};
use toml::Table;
use tree_sitter::{Node, Parser as TreeSitterParser, Query, QueryCursor, Range};
use tree_sitter_md::MarkdownParser;

static LOOKING_GLASS: Emoji<'_, '_> = Emoji("🔍 ", "");
static CROSS_MARK: Emoji<'_, '_> = Emoji("❌ ", "");
static WARNING: Emoji<'_, '_> = Emoji("⚠️ ", "");
static CHECK: Emoji<'_, '_> = Emoji("✅️ ", "");

const UNSPECIFIED_ERROR: i32 = 1;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, value_enum)]
    bump: VersionCoordinate,
    project_dir: String,
}

#[derive(ValueEnum, Debug, Clone)]
enum VersionCoordinate {
    Major,
    Minor,
    Patch,
}

#[derive(Eq, PartialEq)]
struct TargetToPrepare {
    path: PathBuf,
    buildpack_toml: BuildpackToml,
    changelog_md: ChangelogMarkdown,
}

#[derive(Eq, PartialEq)]
struct BuildpackToml {
    path: PathBuf,
    contents: String,
    buildpack_id: BuildpackId,
    current_version: BuildpackVersion,
    current_version_location: Range,
}

#[derive(Eq, PartialEq)]
struct ChangelogMarkdown {
    path: PathBuf,
    contents: String,
    unreleased_changes: String,
    unreleased_changes_location: Range,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let mut targets_to_prepare =
        find_directories_containing_a_buildpack_and_changelog(args.project_dir);

    targets_to_prepare.sort_by(|a, b| {
        let id_a = &a.buildpack_toml.buildpack_id.to_string();
        let id_b = &b.buildpack_toml.buildpack_id.to_string();
        id_a.cmp(id_b)
    });

    let current_version = get_fixed_version(&targets_to_prepare);
    let next_version = calculate_next_version(&current_version, args.bump);
    let unreleased_changes = get_all_unreleased_changes(&targets_to_prepare);

    for target_to_prepare in targets_to_prepare {
        update_buildpack_version_and_changelog(target_to_prepare, &next_version);
    }

    // write output contents to github actions (or fallback to stdout)
    let output_variables = [
        ("from_version", current_version.to_string()),
        ("to_version", next_version.to_string()),
        ("unreleased_changes", unreleased_changes),
    ];
    for (name, value) in output_variables {
        let line = format!("{name}={value}\n");
        match env::var("GITHUB_OUTPUT") {
            Ok(output_file) => write(output_file, line)?,
            Err(_) => print!("{line}"),
        }
    }

    Ok(())
}

fn find_directories_containing_a_buildpack_and_changelog(
    project_dir: String,
) -> Vec<TargetToPrepare> {
    eprintln!("{LOOKING_GLASS} Looking for Buildpacks & Changelogs");
    let project_dir = PathBuf::from(project_dir);

    let buildpack_dirs: HashSet<_> =
        match glob(&project_dir.join("**/buildpack.toml").to_string_lossy()) {
            Ok(paths) => paths
                .filter_map(Result::ok)
                .map(|path| parent_dir(&path))
                .collect(),
            Err(error) => {
                fail_with_error(format!(
                    "Failed to glob buildpack.toml files in {}: {}",
                    project_dir.to_string_lossy(),
                    error
                ));
            }
        };

    let changelog_dirs: HashSet<_> =
        match glob(&project_dir.join("**/CHANGELOG.md").to_string_lossy()) {
            Ok(paths) => paths
                .filter_map(Result::ok)
                .map(|path| parent_dir(&path))
                .collect(),
            Err(error) => {
                fail_with_error(format!(
                    "Failed to glob CHANGELOG.md files in {}: {}",
                    project_dir.to_string_lossy(),
                    error
                ));
            }
        };

    let (dirs_with_a_changelog_and_buildpack, dirs_without): (HashSet<_>, HashSet<_>) =
        buildpack_dirs
            .into_iter()
            .partition(|dir| changelog_dirs.contains(dir));

    for dir_without in dirs_without {
        eprintln!(
            "{WARNING} Ignoring {}: buildpack.toml found but no CHANGELOG.md",
            dir_without.to_string_lossy()
        );
    }

    dirs_with_a_changelog_and_buildpack
        .iter()
        .map(|dir| create_target_to_prepare(dir))
        .collect()
}

fn create_target_to_prepare(dir: &Path) -> TargetToPrepare {
    let buildpack_toml_path = dir.join("buildpack.toml");
    let buildpack_toml_contents = match fs::read_to_string(&buildpack_toml_path) {
        Ok(contents) => contents,
        Err(error) => fail_with_error(format!(
            "Could not read contents of {}: {}",
            buildpack_toml_path.to_string_lossy(),
            error
        )),
    };

    let buildpack: BuildpackDescriptor<Option<Table>> =
        match toml::from_str(&buildpack_toml_contents) {
            Ok(buildpack) => buildpack,
            Err(error) => fail_with_error(format!(
                "Could not deserialize buildpack data from {}: {}",
                buildpack_toml_path.to_string_lossy(),
                error
            )),
        };

    let (buildpack_id, buildpack_version) = match buildpack {
        BuildpackDescriptor::Single(data) => (data.buildpack.id, data.buildpack.version),
        BuildpackDescriptor::Meta(data) => (data.buildpack.id, data.buildpack.version),
    };

    let (extracted_version, range) =
        extract_version_from_buildpack_toml(&buildpack_toml_path, &buildpack_toml_contents);

    if extracted_version != buildpack_version {
        fail_with_error(format!(
            "Could not determine the correct text range to replace in {}",
            buildpack_toml_path.to_string_lossy()
        ));
    }

    let buildpack_toml = BuildpackToml {
        path: buildpack_toml_path,
        contents: buildpack_toml_contents,
        current_version: buildpack_version,
        current_version_location: range,
        buildpack_id,
    };

    let changelog_md_path = dir.join("CHANGELOG.md");
    let changelog_md_contents = match fs::read_to_string(&changelog_md_path) {
        Ok(contents) => contents,
        Err(error) => fail_with_error(format!(
            "Could not read contents of {}: {}",
            changelog_md_path.to_string_lossy(),
            error
        )),
    };
    let (unreleased_changes, range) =
        extract_unreleased_changes_from_changelog_md(&changelog_md_path, &changelog_md_contents);
    let changelog_md = ChangelogMarkdown {
        path: changelog_md_path,
        contents: changelog_md_contents,
        unreleased_changes,
        unreleased_changes_location: range,
    };

    TargetToPrepare {
        path: dir.to_path_buf(),
        buildpack_toml,
        changelog_md,
    }
}

// why not just use toml_edit?
// because it doesn't retain the ordering of toml keys and will rewrite the document entirely :(
// but using an AST lets us identify the exact line that needs to be updated
fn extract_version_from_buildpack_toml(
    path: &Path,
    contents: &String,
) -> (BuildpackVersion, Range) {
    let mut parser = TreeSitterParser::new();
    parser
        .set_language(tree_sitter_toml::language())
        .expect("Treesitter TOML grammar should load");

    let tree = match parser.parse(contents, None) {
        Some(tree) => tree,
        None => fail_with_error(format!("Could not parse {}", path.to_string_lossy())),
    };

    // captures the version entry in the toml document that looks like:
    // [buildpack]
    // version = "x.y.z"
    let query_version_declared_using_table = r#"
        (
          (document
            (table
              (bare_key) @table-name
              (pair
                (bare_key) @property-name
                (string) @version
              )
            )
          )
          (#eq? @table-name "buildpack")
          (#eq? @property-name "version")
        )
    "#;

    // captures the version entry in the toml document that looks like:
    // buildpack.version = "0.0.0"
    let query_version_declared_using_dotted_key = r#"
        (
          (document
            (pair
              (dotted_key
                (bare_key) @table-name
                (bare_key) @property-name
              )
              (string) @version
            )
          )
          (#eq? @table-name "buildpack")
          (#eq? @property-name "version")
        )
    "#;

    // captures the version entry in the toml document that looks like:
    // buildpack = { id = "test", version = "0.0.0" }
    let query_version_declared_using_inline_table = r#"
        (
          (document
            (pair 
              (bare_key) @table-name
              (inline_table
                (pair
                  (bare_key) @property-name
                  (string) @version
                )
              )
            )
          )
          (#eq? @table-name "buildpack")
          (#eq? @property-name "version")
        )
    "#;

    let queries = [
        query_version_declared_using_table,
        query_version_declared_using_dotted_key,
        query_version_declared_using_inline_table,
    ];

    for query in queries {
        let query_results = query_toml_ast(query, tree.root_node(), contents.as_bytes());
        if let Some(node) = query_results.get("version") {
            let range = node.range();
            // toml strings are quoted so we want to remove those to get the inner value
            let value = String::from(&contents[range.start_byte + 1..range.end_byte - 1]);
            let version = match BuildpackVersion::try_from(value.clone()) {
                Ok(parsed_version) => parsed_version,
                Err(error) => fail_with_error(format!(
                    "Version {} from {} is invalid: {}",
                    value,
                    path.to_string_lossy(),
                    error
                )),
            };
            return (version, range);
        }
    }

    fail_with_error(format!("No version found in {}", path.to_string_lossy()));
}

fn query_toml_ast<'a>(query: &str, node: Node<'a>, source: &[u8]) -> HashMap<String, Node<'a>> {
    let query = match Query::new(tree_sitter_toml::language(), query) {
        Ok(query) => query,
        Err(error) => fail_with_error(
            format!("TOML AST query is invalid: {query}\n\nError: {error}").as_str(),
        ),
    };
    query_ast(query, node, source)
}

fn extract_unreleased_changes_from_changelog_md(path: &Path, contents: &String) -> (String, Range) {
    let mut parser = MarkdownParser::default();

    let tree = match parser.parse(contents.as_bytes(), None) {
        Some(tree) => tree,
        None => fail_with_error(format!("Could not parse {}", path.to_string_lossy())),
    };

    let mut cursor = tree.walk();

    'outer: loop {
        let node = cursor.node();

        if node.kind() == "atx_heading" {
            let header_range = node.range();
            let is_unreleased_section = contents[header_range.start_byte..header_range.end_byte]
                .to_ascii_lowercase()
                .trim()
                .ends_with("[unreleased]");

            // we want the contents between the [Unreleased] header and the following one
            if is_unreleased_section {
                let mut ranges: Vec<Range> = vec![];
                // loop through each subsequent sibling in the section to collect the ranges
                // of our content nodes
                loop {
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                    ranges.push(cursor.node().range());
                }

                let empty_content = (
                    String::from(""),
                    Range {
                        start_byte: header_range.end_byte,
                        start_point: header_range.end_point,
                        end_byte: header_range.end_byte,
                        end_point: header_range.end_point,
                    },
                );

                return if let Some(first_range) = ranges.first() {
                    if let Some(last_range) = ranges.last() {
                        let range = Range {
                            start_byte: first_range.start_byte,
                            start_point: first_range.start_point,
                            end_byte: last_range.end_byte,
                            end_point: last_range.end_point,
                        };
                        let value = &contents[range.start_byte..range.end_byte];
                        (value.to_string(), range)
                    } else {
                        empty_content
                    }
                } else {
                    empty_content
                };
            }
        }

        // traverse the AST
        if !cursor.goto_first_child() {
            loop {
                if !cursor.goto_parent() {
                    break 'outer;
                }
                if cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fail_with_error(format!("No version found in {}", path.to_string_lossy()));
}

fn query_ast<'a>(query: Query, node: Node<'a>, source: &[u8]) -> HashMap<String, Node<'a>> {
    let mut query_results: HashMap<String, Node> = HashMap::new();
    let mut query_cursor = QueryCursor::new();
    let capture_names = query.capture_names();
    let query_matches = query_cursor.matches(&query, node, source);
    for query_match in query_matches {
        for capture in query_match.captures {
            let capture_name = &capture_names[capture.index as usize];
            query_results.insert(capture_name.clone(), capture.node);
        }
    }
    query_results
}

fn get_fixed_version(targets_to_prepare: &[TargetToPrepare]) -> BuildpackVersion {
    let all_versions: HashSet<_> = targets_to_prepare
        .iter()
        .map(|target_to_prepare| target_to_prepare.buildpack_toml.current_version.to_string())
        .collect();

    if all_versions.len() != 1 {
        fail_with_error(format!(
            "Not all versions match:\n{}",
            targets_to_prepare
                .iter()
                .map(|target_to_prepare| {
                    format!(
                        "• {} ({})",
                        target_to_prepare.buildpack_toml.path.to_string_lossy(),
                        target_to_prepare.buildpack_toml.current_version
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    let target_to_prepare = targets_to_prepare
        .first()
        .expect("There should only be one");

    BuildpackVersion {
        ..target_to_prepare.buildpack_toml.current_version
    }
}

fn calculate_next_version(
    current_version: &BuildpackVersion,
    coordinate: VersionCoordinate,
) -> BuildpackVersion {
    let BuildpackVersion {
        major,
        minor,
        patch,
        ..
    } = current_version;

    match coordinate {
        VersionCoordinate::Major => BuildpackVersion {
            major: major + 1,
            minor: 0,
            patch: 0,
        },
        VersionCoordinate::Minor => BuildpackVersion {
            major: *major,
            minor: minor + 1,
            patch: 0,
        },
        VersionCoordinate::Patch => BuildpackVersion {
            major: *major,
            minor: *minor,
            patch: patch + 1,
        },
    }
}

fn get_all_unreleased_changes(targets_to_prepare: &[TargetToPrepare]) -> String {
    let all_unreleased_changes = targets_to_prepare
        .iter()
        .map(|target_to_prepare| {
            let id = target_to_prepare.buildpack_toml.buildpack_id.clone();
            let unreleased_changes =
                get_unreleased_changes_or_empty_message(&target_to_prepare.changelog_md);
            format!("## {id}\n\n{unreleased_changes}")
        })
        .collect::<Vec<String>>()
        .join("\n\n")
        .trim()
        .to_string();
    format!("{all_unreleased_changes}\n")
}

fn update_buildpack_version_and_changelog(
    target_to_prepare: TargetToPrepare,
    version: &BuildpackVersion,
) {
    update_buildpack_toml(&target_to_prepare.buildpack_toml, version);
    update_changelog_md(&target_to_prepare.changelog_md, version);
}

fn update_buildpack_toml(buildpack_toml: &BuildpackToml, version: &BuildpackVersion) {
    eprintln!(
        "{CHECK} Updating version {} → {}: {}",
        buildpack_toml.current_version,
        version,
        buildpack_toml.path.to_string_lossy(),
    );

    let new_contents = format!(
        "{}\"{}\"{}",
        &buildpack_toml.contents[..buildpack_toml.current_version_location.start_byte],
        version,
        &buildpack_toml.contents[buildpack_toml.current_version_location.end_byte..]
    );

    if let Err(error) = write(&buildpack_toml.path, new_contents) {
        fail_with_error(format!(
            "Could not write to {}: {error}",
            &buildpack_toml.path.to_string_lossy()
        ));
    }
}

fn update_changelog_md(changelog_md: &ChangelogMarkdown, version: &BuildpackVersion) {
    let formatted_date = Utc::now().format("%Y-%m-%d").to_string();
    let new_header = format!("## [{version}] {formatted_date}");
    let unreleased_changes = get_unreleased_changes_or_empty_message(changelog_md);

    eprintln!(
        "{CHECK} Adding changelog entry \"{new_header}: {}",
        changelog_md.path.to_string_lossy()
    );

    let entry = format!("{new_header}\n\n{unreleased_changes}");
    let contents_before =
        &changelog_md.contents[..changelog_md.unreleased_changes_location.start_byte];
    let contents_after =
        &changelog_md.contents[changelog_md.unreleased_changes_location.end_byte..];

    let new_contents = format!(
        "{}\n\n{}\n\n{}",
        contents_before.trim_end(),
        entry.trim(),
        contents_after.trim_start()
    );

    if let Err(error) = write(&changelog_md.path, format!("{}\n", new_contents.trim_end())) {
        fail_with_error(format!(
            "Could not write to {}: {error}",
            &changelog_md.path.to_string_lossy()
        ));
    }
}

fn get_unreleased_changes_or_empty_message(changelog_md: &ChangelogMarkdown) -> String {
    let text_or_empty_message = if changelog_md.unreleased_changes.trim().is_empty() {
        "- No Changes"
    } else {
        changelog_md.unreleased_changes.trim()
    };
    text_or_empty_message.to_string()
}

fn parent_dir(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        parent.to_path_buf()
    } else {
        fail_with_error(format!(
            "Could not get parent directory from {}",
            path.to_string_lossy()
        ));
    }
}

fn fail_with_error<IntoString: Into<String>>(error: IntoString) -> ! {
    eprintln!("{CROSS_MARK} {}", error.into());
    std::process::exit(UNSPECIFIED_ERROR);
}

#[cfg(test)]
mod test {
    use crate::{
        extract_unreleased_changes_from_changelog_md, extract_version_from_buildpack_toml,
    };
    use std::path::Path;

    #[test]
    fn test_query_version_from_toml_table() {
        let toml = r#"
[buildpack]
id = "test"
version = "0.0.0"

[[order]]

[[order.group]]
id = "a"
version = "0.0.1"

[[order.group]]
id = "b"
version = "0.0.2"
        "#
        .to_string();

        let (version, range) = extract_version_from_buildpack_toml(Path::new("/test/path"), &toml);
        assert_eq!("0.0.0", version.to_string());
        assert_eq!(
            "\"0.0.0\"",
            toml[range.start_byte..range.end_byte].to_string()
        );
    }

    #[test]
    fn test_query_version_from_toml_with_dotted_key() {
        let toml = r#"
buildpack.id = "test"
buildpack.version = "0.0.0"

[[order]]

[[order.group]]
id = "a"
version = "0.0.1"

[[order.group]]
id = "b"
version = "0.0.2"
        "#
        .to_string();

        let (version, range) = extract_version_from_buildpack_toml(Path::new("/test/path"), &toml);
        assert_eq!("0.0.0", version.to_string());
        assert_eq!(
            "\"0.0.0\"",
            toml[range.start_byte..range.end_byte].to_string()
        );
    }

    #[test]
    fn test_query_version_from_toml_with_inline_table() {
        let toml = r#"
buildpack = { 
  id = "test", 
  version = "0.0.0" 
}

[[order]]

[[order.group]]
id = "a"
version = "0.0.1"

[[order.group]]
id = "b"
version = "0.0.2"
        "#
        .to_string();

        let (version, range) = extract_version_from_buildpack_toml(Path::new("/test/path"), &toml);
        assert_eq!("0.0.0", version.to_string());
        assert_eq!(
            "\"0.0.0\"",
            toml[range.start_byte..range.end_byte].to_string()
        );
    }

    #[test]
    fn test_query_unreleased_changes_from_changelog_with_existing_entries() {
        let changelog = r#"
# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Added node version 18.15.0.

## [0.8.16] 2023/02/27

- Added node version 19.7.0, 19.6.1, 14.21.3, 16.19.1, 18.14.1, 18.14.2.
- Added node version 18.14.0, 19.6.0.

## [0.8.15] 2023/02/02

- `name` is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))
- Added node version 19.5.0.
        "#
        .trim().to_string();

        let (unreleased_changes, range) =
            extract_unreleased_changes_from_changelog_md(Path::new("/test/path"), &changelog);
        assert_eq!("- Added node version 18.15.0.\n\n", unreleased_changes);
        assert_eq!(
            "- Added node version 18.15.0.\n\n",
            changelog[range.start_byte..range.end_byte].to_string()
        );
    }

    #[test]
    fn test_query_unreleased_changes_from_changelog_with_existing_entries_no_spacing() {
        let changelog = r#"
# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
- Added node version 18.15.0.
## [0.8.16] 2023/02/27

- Added node version 19.7.0, 19.6.1, 14.21.3, 16.19.1, 18.14.1, 18.14.2.
- Added node version 18.14.0, 19.6.0.

## [0.8.15] 2023/02/02

- `name` is no longer a required field in package.json. ([#447](https://github.com/heroku/buildpacks-nodejs/pull/447))
- Added node version 19.5.0.
        "#
            .trim().to_string();

        let (unreleased_changes, range) =
            extract_unreleased_changes_from_changelog_md(Path::new("/test/path"), &changelog);
        assert_eq!("- Added node version 18.15.0.\n", unreleased_changes);
        assert_eq!(
            "- Added node version 18.15.0.\n",
            changelog[range.start_byte..range.end_byte].to_string()
        );
    }

    #[test]
    fn test_query_unreleased_changes_from_changelog_with_no_entries() {
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
        "#
            .trim().to_string();

        let (unreleased_changes, range) =
            extract_unreleased_changes_from_changelog_md(Path::new("/test/path"), &changelog);
        assert_eq!("", unreleased_changes);
        assert_eq!("", changelog[range.start_byte..range.end_byte].to_string());
    }

    #[test]
    fn test_query_unreleased_changes_from_changelog_initial_state() {
        let changelog = r#"
# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
        "#
        .trim()
        .to_string();

        let (unreleased_changes, range) =
            extract_unreleased_changes_from_changelog_md(Path::new("/test/path"), &changelog);
        assert_eq!("", unreleased_changes);
        assert_eq!("", changelog[range.start_byte..range.end_byte].to_string());
    }
}
