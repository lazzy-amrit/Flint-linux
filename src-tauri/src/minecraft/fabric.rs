use super::{download, install::PreparedVersion, metadata::Arguments};
use crate::{
    error::{AppError, Result},
    paths::AppPaths,
};
use serde::{Deserialize, Serialize};

const FABRIC_META: &str = "https://meta.fabricmc.net/v2";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricLoaderVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Deserialize)]
struct LoaderResponse {
    loader: LoaderInfo,
}

#[derive(Debug, Deserialize)]
struct LoaderInfo {
    version: String,
    stable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FabricProfile {
    id: String,
    main_class: String,
    #[serde(default)]
    arguments: Option<Arguments>,
    libraries: Vec<FabricLibrary>,
}

#[derive(Debug, Deserialize)]
struct FabricLibrary {
    name: String,
    url: String,
    #[serde(default)]
    sha1: String,
    #[serde(default)]
    size: u64,
}

pub async fn list_loaders(game_version: &str) -> Result<Vec<FabricLoaderVersion>> {
    let client = client()?;
    let response: Vec<LoaderResponse> = client
        .get(format!("{FABRIC_META}/versions/loader/{game_version}"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(response
        .into_iter()
        .map(|item| FabricLoaderVersion {
            version: item.loader.version,
            stable: item.loader.stable,
        })
        .collect())
}

pub async fn apply(
    paths: &AppPaths,
    game_version: &str,
    loader_version: &str,
    prepared: &mut PreparedVersion,
) -> Result<()> {
    let client = client()?;
    let profile: FabricProfile = client
        .get(format!(
            "{FABRIC_META}/versions/loader/{game_version}/{loader_version}/profile/json"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    for library in profile.libraries {
        let relative = maven_path(&library.name)?;
        let url = format!("{}/{relative}", library.url.trim_end_matches('/'));
        let sha1 = if library.sha1.is_empty() {
            client
                .get(format!("{url}.sha1"))
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?
                .trim()
                .to_owned()
        } else {
            library.sha1
        };
        let target = paths.libraries.join(&relative);
        download::ensure(
            &client,
            &format!("Fabric library {}", library.name),
            &url,
            &sha1,
            library.size,
            &target,
        )
        .await?;
        prepared.classpath.push(target);
    }
    prepared.metadata.id = profile.id;
    prepared.metadata.main_class = profile.main_class;
    if let Some(fabric_arguments) = profile.arguments {
        let arguments = prepared
            .metadata
            .arguments
            .get_or_insert_with(|| Arguments {
                game: Vec::new(),
                jvm: Vec::new(),
            });
        arguments.jvm.extend(fabric_arguments.jvm);
        arguments.game.extend(fabric_arguments.game);
    }
    Ok(())
}

fn client() -> Result<reqwest::Client> {
    download::client_builder()
        .user_agent(concat!("Flint/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(Into::into)
}

fn maven_path(coordinate: &str) -> Result<String> {
    let parts = coordinate.split(':').collect::<Vec<_>>();
    if parts.len() < 3 || parts.len() > 4 {
        return Err(AppError::new(
            "invalid_fabric_library",
            format!("Fabric supplied an unsupported Maven coordinate: {coordinate}"),
        ));
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts
        .get(3)
        .map(|value| format!("-{value}"))
        .unwrap_or_default();
    Ok(format!(
        "{group}/{artifact}/{version}/{artifact}-{version}{classifier}.jar"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_maven_coordinate_to_repository_path() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.19.5").unwrap(),
            "net/fabricmc/fabric-loader/0.19.5/fabric-loader-0.19.5.jar"
        );
    }

    #[tokio::test]
    async fn live_fabric_metadata_when_enabled() {
        if std::env::var_os("FLINT_LIVE_TEST").is_none() {
            return;
        }
        let loaders = list_loaders("26.2").await.unwrap();
        let loader = loaders.iter().find(|item| item.stable).unwrap();
        let profile: FabricProfile = client()
            .unwrap()
            .get(format!(
                "{FABRIC_META}/versions/loader/26.2/{}/profile/json",
                loader.version
            ))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(profile.main_class.contains("KnotClient"));
        assert!(!profile.libraries.is_empty());
    }
}
