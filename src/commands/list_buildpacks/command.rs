use crate::commands::list_buildpacks::errors::Error;
use crate::github::actions;
use libcnb_package::find_buildpack_dirs;

pub(crate) fn execute() -> Result<()> {
    let current_dir = std::env::current_dir().map_err(Error::GetCurrentDir)?;
    let buildpack_dirs = find_buildpack_dirs(&current_dir)?;
    let buildpack_dirs_as_json_array = format!(
        "[{}]",
        buildpack_dirs
            .into_iter()
            .filter_map(|dir| {
                if dir.starts_with(current_dir.join("target")) {
                    None
                } else {
                    Some(format!("\"{}\"", dir.display()))
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    );
    actions::set_output("buildpacks", buildpack_dirs_as_json_array)?;
    Ok(())
}

type Result<T> = std::result::Result<T, Error>;
