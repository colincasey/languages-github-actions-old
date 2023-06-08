use crate::commands::generate_changelog::errors::Error;
use crate::commands::{read_changelog_file_from_dir, ChangelogEntrySection};
use crate::github::actions;
use clap::Parser;
use libcnb_data::buildpack::BuildpackId;
use libcnb_package::{find_buildpack_dirs, read_buildpack_data, FindBuildpackDirsOptions};
use std::collections::{BTreeMap, HashMap};

type Result<T> = std::result::Result<T, Error>;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None, disable_version_flag = true)]
pub(crate) struct GenerateChangelogArgs {
    #[arg(long, group = "section")]
    unreleased: bool,
    #[arg(long, group = "section")]
    version: Option<String>,
}

pub(crate) fn execute(args: GenerateChangelogArgs) -> Result<()> {
    let current_dir = std::env::current_dir().map_err(Error::GetCurrentDir)?;

    let find_buildpack_dirs_options = FindBuildpackDirsOptions {
        ignore: vec![current_dir.join("target")],
    };

    let buildpack_dirs = find_buildpack_dirs(&current_dir, &find_buildpack_dirs_options)
        .map_err(Error::FindingBuildpacks)?;

    let changes_by_buildpack = buildpack_dirs
        .iter()
        .map(|dir| {
            read_buildpack_data(dir)
                .map_err(Error::GetBuildpackId)
                .map(|data| data.buildpack_descriptor.buildpack().id.clone())
                .and_then(|buildpack_id| {
                    read_changelog_file_from_dir(dir)
                        .map_err(Error::ChangelogFile)
                        .map(|changelog| {
                            (
                                buildpack_id,
                                match &args.version {
                                    Some(version) => {
                                        changelog.parsed.versions.get(version).cloned()
                                    }
                                    None => Some(&changelog.parsed.unreleased).cloned(),
                                },
                            )
                        })
                })
        })
        .collect::<Result<HashMap<_, _>>>()?;

    let changelog = generate_changelog(&changes_by_buildpack);

    actions::set_output("changelog", changelog).map_err(Error::SetActionOutput)?;

    Ok(())
}

fn generate_changelog(
    changes_by_buildpack: &HashMap<BuildpackId, Option<ChangelogEntrySection>>,
) -> String {
    let changelog = changes_by_buildpack
        .iter()
        .map(|(buildpack_id, changes)| (buildpack_id.to_string(), changes))
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .filter_map(|(buildpack_id, changes)| {
            changes
                .as_ref()
                .map(|section| format!("# {buildpack_id}\n\n{section}"))
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{}\n\n", changelog.trim())
}

#[cfg(test)]
mod test {
    use crate::commands::generate_changelog::command::generate_changelog;
    use crate::commands::ChangelogEntrySection;
    use libcnb_data::buildpack_id;
    use std::collections::HashMap;

    #[test]
    fn test_generating_changelog() {
        let values = HashMap::from([
            (
                buildpack_id!("c"),
                Some(ChangelogEntrySection {
                    span: 0..0,
                    value: Some("- change c.1".to_string()),
                }),
            ),
            (
                buildpack_id!("a"),
                Some(ChangelogEntrySection {
                    span: 0..0,
                    value: Some("- change a.1\n- change a.2".to_string()),
                }),
            ),
            (buildpack_id!("b"), None),
            (
                buildpack_id!("d"),
                Some(ChangelogEntrySection {
                    span: 0..0,
                    value: None,
                }),
            ),
        ]);

        assert_eq!(
            generate_changelog(&values),
            r#"# a

- change a.1
- change a.2

# c

- change c.1

# d

- No Changes

"#
        )
    }
}
