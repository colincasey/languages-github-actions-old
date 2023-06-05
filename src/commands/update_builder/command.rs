use crate::cli::UpdateBuilderArgs;
use crate::update_builder::errors::Error;
use libcnb_data::buildpack::{BuildpackId, BuildpackVersion};
use serde::Deserialize;
use std::path::PathBuf;
use toml::Spanned;
use uriparse::URIReference;

pub(crate) fn execute(args: UpdateBuilderArgs) -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(Error::GetCurrentDir)
        .map(|dir| dir.join(PathBuf::from(args.path)))?;

    let buildpack_id = args.buildpack_id;

    let buildpack_uri = URIReference::try_from(args.buildpack_uri.as_str())
        .map_err(|e| Error::InvalidBuildpackUri(args.buildpack_uri.clone(), e))?;

    let buildpack_version = BuildpackVersion::try_from(args.buildpack_version.to_string())
        .map_err(|e| Error::InvalidBuildpackVersion(args.buildpack_version, e))?;

    let builder_files = args
        .builders
        .iter()
        .map(|builder| {
            let path = current_dir.join(builder).join("builder.toml");
            std::fs::read_to_string(&path)
                .map_err(|e| Error::ReadingBuilder(path.clone(), e))
                .and_then(|raw| {
                    toml::from_str(&raw)
                        .map_err(|e| Error::ParsingBuilder(path.clone(), e))
                        .map(|parsed| BuilderFile { path, raw, parsed })
                })
        })
        .collect::<Result<Vec<_>>>()?;

    if builder_files.is_empty() {
        Err(Error::NoBuilderFiles(args.builders))?;
    }

    for builder_file in builder_files {
        std::fs::write(
            &builder_file.path,
            update_builder_contents_with_buildpack(
                &builder_file,
                &buildpack_id,
                &buildpack_version,
                &buildpack_uri,
            ),
        )
        .map_err(|e| Error::WritingBuilder(builder_file.path, e))?;
    }

    Ok(())
}

fn update_builder_contents_with_buildpack(
    builder_file: &BuilderFile,
    buildpack_id: &BuildpackId,
    buildpack_version: &BuildpackVersion,
    buildpack_uri: &URIReference,
) -> String {
    let mut new_contents = builder_file.raw.to_string();

    for buildpack in &builder_file.parsed.buildpacks {
        if &buildpack.id == buildpack_id {
            new_contents = format!(
                "{}\"{}\"{}",
                &new_contents[..buildpack.uri.span().start],
                buildpack_uri,
                &new_contents[buildpack.uri.span().end..]
            )
        }
    }

    for order_item in &builder_file.parsed.order {
        for group_item in &order_item.group {
            if &group_item.id == buildpack_id {
                new_contents = format!(
                    "{}\"{}\"{}",
                    &new_contents[..group_item.version.span().start],
                    buildpack_version,
                    &new_contents[group_item.version.span().end..]
                );
            }
        }
    }

    new_contents
}

type Result<T> = std::result::Result<T, Error>;

struct BuilderFile {
    path: PathBuf,
    raw: String,
    parsed: MinimalBuilder,
}

#[derive(Debug, Deserialize)]
struct MinimalBuilder {
    buildpacks: Vec<BuilderBuildpack>,
    order: Vec<BuilderOrder>,
}

#[derive(Debug, Deserialize)]
struct BuilderBuildpack {
    id: BuildpackId,
    uri: Spanned<String>,
}

#[derive(Debug, Deserialize)]
struct BuilderOrder {
    group: Vec<BuilderOrderGroup>,
}

#[derive(Debug, Deserialize)]
struct BuilderOrderGroup {
    id: BuildpackId,
    version: Spanned<BuildpackVersion>,
}

#[cfg(test)]
mod test {
    use crate::commands::update_builder::command::{
        update_builder_contents_with_buildpack, BuilderFile,
    };
    use libcnb_data::buildpack::BuildpackVersion;
    use libcnb_data::buildpack_id;
    use std::path::PathBuf;
    use uriparse::URIReference;

    #[test]
    fn test_update_builder_contents_with_buildpack() {
        let toml = r#"
[[buildpacks]]
  id = "heroku/java"
  uri = "docker://docker.io/heroku/buildpack-java@sha256:21990393c93927b16f76c303ae007ea7e95502d52b0317ca773d4cd51e7a5682"

[[buildpacks]]
  id = "heroku/nodejs"
  uri = "docker://docker.io/heroku/buildpack-nodejs@sha256:22ec91eebee2271b99368844f193c4bb3c6084201062f89b3e45179b938c3241"

[[order]]
  [[order.group]]
    id = "heroku/nodejs"
    version = "0.6.5"  

[[order]]
  [[order.group]]
    id = "heroku/java"
    version = "0.6.9"

  [[order.group]]
    id = "heroku/procfile"
    version = "2.0.0"
    optional = true
"#;
        let builder_file = create_builder_file(toml);
        assert_eq!(
            update_builder_contents_with_buildpack(
                &builder_file,
                &buildpack_id!("heroku/java"),
                &BuildpackVersion::try_from("0.6.10".to_string()).unwrap(),
                &URIReference::try_from("docker://docker.io/heroku/buildpack-java@sha256:c6dd500be06a2a1e764c30359c5dd4f4955a98b572ef3095b2f6115cd8a87c99").unwrap()
            ),
            r#"
[[buildpacks]]
  id = "heroku/java"
  uri = "docker://docker.io/heroku/buildpack-java@sha256:c6dd500be06a2a1e764c30359c5dd4f4955a98b572ef3095b2f6115cd8a87c99"

[[buildpacks]]
  id = "heroku/nodejs"
  uri = "docker://docker.io/heroku/buildpack-nodejs@sha256:22ec91eebee2271b99368844f193c4bb3c6084201062f89b3e45179b938c3241"

[[order]]
  [[order.group]]
    id = "heroku/nodejs"
    version = "0.6.5"  

[[order]]
  [[order.group]]
    id = "heroku/java"
    version = "0.6.10"

  [[order.group]]
    id = "heroku/procfile"
    version = "2.0.0"
    optional = true
"#
        )
    }

    fn create_builder_file(contents: &str) -> BuilderFile {
        BuilderFile {
            path: PathBuf::from("/path/to/builder.toml"),
            raw: contents.to_string(),
            parsed: toml::from_str(contents).unwrap(),
        }
    }
}
