use super::{install::PreparedVersion, metadata::resolved_argument};
use crate::{
    error::{AppError, Result},
    paths::AppPaths,
    profiles::Profile,
};
use std::{collections::HashMap, path::Path};
use uuid::Uuid;

#[cfg(target_os = "windows")]
const CLASSPATH_SEPARATOR: &str = ";";
#[cfg(target_os = "linux")]
const CLASSPATH_SEPARATOR: &str = ":";

pub struct LaunchArguments {
    pub jvm: Vec<String>,
    pub game: Vec<String>,
}

pub fn build(
    prepared: &PreparedVersion,
    paths: &AppPaths,
    profile: &Profile,
    game_dir: &Path,
    resolution: (u32, u32),
) -> Result<LaunchArguments> {
    let classpath = prepared
        .classpath
        .iter()
        .chain(std::iter::once(&prepared.client_jar))
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join(CLASSPATH_SEPARATOR);
    let offline_uuid = Uuid::new_v3(
        &Uuid::NAMESPACE_DNS,
        format!("OfflinePlayer:{}", profile.username).as_bytes(),
    )
    .simple()
    .to_string();
    let values: HashMap<&str, String> = HashMap::from([
        ("${auth_player_name}", profile.username.clone()),
        ("${version_name}", prepared.metadata.id.clone()),
        ("${game_directory}", game_dir.to_string_lossy().into_owned()),
        (
            "${assets_root}",
            paths.assets.to_string_lossy().into_owned(),
        ),
        ("${assets_index_name}", prepared.metadata.assets.clone()),
        ("${auth_uuid}", offline_uuid),
        ("${auth_access_token}", "0".into()),
        ("${auth_session}", "0".into()),
        ("${user_type}", "legacy".into()),
        ("${version_type}", "release".into()),
        ("${resolution_width}", resolution.0.to_string()),
        ("${resolution_height}", resolution.1.to_string()),
        ("${user_properties}", "{}".into()),
        ("${clientid}", String::new()),
        ("${xuid}", String::new()),
        ("${auth_xuid}", String::new()),
        (
            "${natives_directory}",
            prepared.natives_dir.to_string_lossy().into_owned(),
        ),
        ("${launcher_name}", "Flint".into()),
        ("${launcher_version}", env!("CARGO_PKG_VERSION").into()),
        ("${classpath}", classpath.clone()),
        (
            "${classpath_separator}",
            CLASSPATH_SEPARATOR.into(),
        ),
        (
            "${library_directory}",
            paths.libraries.to_string_lossy().into_owned(),
        ),
    ]);
    let mut jvm = Vec::new();
    let mut game = Vec::new();
    if let Some(arguments) = &prepared.metadata.arguments {
        for argument in &arguments.jvm {
            jvm.extend(
                resolved_argument(argument)
                    .into_iter()
                    .map(|value| substitute(value, &values)),
            );
        }
        for argument in &arguments.game {
            game.extend(
                resolved_argument(argument)
                    .into_iter()
                    .map(|value| substitute(value, &values)),
            );
        }
    } else if let Some(legacy) = &prepared.metadata.minecraft_arguments {
        game.extend(
            legacy
                .split_whitespace()
                .map(|value| substitute(value.into(), &values)),
        );
    } else {
        return Err(AppError::new(
            "missing_arguments",
            "Minecraft metadata did not contain launch arguments.",
        ));
    }
    if !jvm
        .iter()
        .any(|value| value == "-cp" || value == "-classpath")
    {
        jvm.extend(["-cp".into(), classpath]);
    }
    jvm.insert(0, format!("-Xmx{}M", profile.memory_mb));
    if let (Some(logging), Some(path)) = (&prepared.metadata.logging, &prepared.logging_config) {
        jvm.push(
            logging
                .client
                .argument
                .replace("${path}", &path.to_string_lossy()),
        );
    }
    Ok(LaunchArguments { jvm, game })
}

fn substitute(mut value: String, values: &HashMap<&str, String>) -> String {
    for (placeholder, replacement) in values {
        value = value.replace(placeholder, replacement);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replaces_all_known_placeholders() {
        let values = HashMap::from([
            ("${name}", "Flint".to_string()),
            ("${version}", "1".to_string()),
        ]);
        assert_eq!(substitute("${name}-${version}".into(), &values), "Flint-1");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_uses_colon_for_classpaths() {
        assert_eq!(CLASSPATH_SEPARATOR, ":");
    }
}
