use crate::commands::generate_buildpack_matrix::errors::Error;
use crate::github::actions;
use libcnb_package::{find_buildpack_dirs, read_buildpack_data, FindBuildpackDirsOptions};
use std::collections::HashMap;

pub(crate) fn execute() -> Result<()> {
    let current_dir = std::env::current_dir().map_err(Error::GetCurrentDir)?;

    let find_buildpack_dirs_options = FindBuildpackDirsOptions {
        ignore: vec![current_dir.join("target")],
    };

    let buildpacks = find_buildpack_dirs(&current_dir, &find_buildpack_dirs_options)?
        .into_iter()
        .map(|dir| {
            read_buildpack_data(&dir)
                .map_err(Error::ReadingBuildpack)
                .map(|data| {
                    HashMap::from([
                        ("id", data.buildpack_descriptor.buildpack().id.to_string()),
                        ("path", dir.to_string_lossy().to_string()),
                    ])
                })
        })
        .collect::<Result<Vec<_>>>()?;

    let json = serde_json::to_string(&buildpacks)?;
    actions::set_output("buildpacks", json)?;

    Ok(())
}

type Result<T> = std::result::Result<T, Error>;
