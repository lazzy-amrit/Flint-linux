use super::{download, metadata::*};
use crate::{
    error::{AppError, Result},
    paths::AppPaths,
};
use futures::{stream, StreamExt, TryStreamExt};
use serde::Serialize;
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Debug, Serialize)]
pub struct LauncherStatus {
    pub phase: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
}

pub struct PreparedVersion {
    pub metadata: VersionMetadata,
    pub client_jar: PathBuf,
    pub classpath: Vec<PathBuf>,
    pub natives_dir: PathBuf,
    pub logging_config: Option<PathBuf>,
}

pub struct ResolvedVersion {
    pub metadata: VersionMetadata,
    pub version_dir: PathBuf,
}

pub fn emit(
    app: &AppHandle,
    phase: &'static str,
    message: impl Into<String>,
    progress: Option<f64>,
) {
    let message = message.into();
    tracing::info!(phase, %message, "launcher status");
    let _ = app.emit(
        "launcher-status",
        LauncherStatus {
            phase,
            message,
            detail: None,
            progress,
        },
    );
}

pub async fn resolve(paths: &AppPaths, version: &str) -> Result<ResolvedVersion> {
    let client = download::client_builder()
        .user_agent(concat!("Flint/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let manifest = super::catalog::load_manifest(paths).await?;
    let reference = manifest
        .versions
        .into_iter()
        .find(|item| item.id == version)
        .ok_or_else(|| {
            AppError::new(
                "version_unavailable",
                format!("Minecraft {version} was not found in Mojang's version manifest."),
            )
        })?;
    let version_dir = paths.versions.join(version);
    let metadata_path = version_dir.join(format!("{version}.json"));
    download::ensure(
        &client,
        &format!("Minecraft {version} version metadata"),
        &reference.url,
        &reference.sha1,
        0,
        &metadata_path,
    )
    .await?;
    let metadata: VersionMetadata =
        serde_json::from_slice(&tokio::fs::read(&metadata_path).await?)?;
    if metadata.id != version {
        return Err(AppError::new(
            "version_mismatch",
            "Mojang returned metadata for a different Minecraft version.",
        ));
    }
    metadata
        .java_version
        .as_ref()
        .map(|java| java.major_version)
        .ok_or_else(|| {
            AppError::new(
                "java_requirement_missing",
                "Minecraft metadata did not declare a required Java version.",
            )
        })?;
    Ok(ResolvedVersion {
        metadata,
        version_dir,
    })
}

pub async fn prepare(
    app: &AppHandle,
    paths: &AppPaths,
    resolved: ResolvedVersion,
) -> Result<PreparedVersion> {
    let client = download::client_builder()
        .user_agent(concat!("Flint/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let metadata = resolved.metadata;
    let version_dir = resolved.version_dir;
    let version = &metadata.id;

    let client_jar = version_dir.join(format!("{version}.jar"));
    emit(
        app,
        "downloading",
        "Preparing Minecraft client and libraries…",
        Some(0.05),
    );
    download::ensure(
        &client,
        &format!("Minecraft {version} client JAR"),
        &metadata.downloads.client.url,
        &metadata.downloads.client.sha1,
        metadata.downloads.client.size,
        &client_jar,
    )
    .await?;

    let natives_dir = version_dir.join("natives");
    tokio::fs::create_dir_all(&natives_dir).await?;
    let mut classpath = Vec::new();
    let library_total = metadata
        .libraries
        .iter()
        .filter(|library| rules_allow(library.rules.as_deref()))
        .count();
    let mut library_count = 0;
    for library in &metadata.libraries {
        if !rules_allow(library.rules.as_deref()) {
            continue;
        }
        library_count += 1;
        if library_count == library_total || library_count % 10 == 0 {
            emit(
                app,
                "downloading",
                format!(
                    "Preparing libraries ({library_count}/{library_total}): {}",
                    library.name
                ),
                Some(0.05 + 0.15 * library_count as f64 / library_total.max(1) as f64),
            );
        }
        if let Some(artifact) = &library.downloads.artifact {
            let relative = artifact.path.as_ref().ok_or_else(|| {
                AppError::new(
                    "invalid_library",
                    format!("Library {} has no path.", library.name),
                )
            })?;
            let target = paths.libraries.join(relative);
            download::ensure(
                &client,
                &format!("library {}", library.name),
                &artifact.url,
                &artifact.sha1,
                artifact.size,
                &target,
            )
            .await?;
            if native_artifact_matches_platform(&library.name) {
                let family = if library.name.starts_with("org.lwjgl:") {
                    "lwjgl"
                } else if library.name.starts_with("io.netty:") {
                    "netty"
                } else if library.name.starts_with("net.java.dev.jna:") {
                    "jna"
                } else {
                    "java"
                };
                extract_native(&target, &natives_dir.join(family), library.extract.as_ref())?;
            }
            classpath.push(target);
        }
        if let Some(template) = native_template(&library) {
            let arch = if cfg!(target_arch = "x86_64") {
                "64"
            } else {
                "32"
            };
            let classifier = template.replace("${arch}", arch);
            let native = library
                .downloads
                .classifiers
                .get(&classifier)
                .ok_or_else(|| {
                    AppError::new(
                        "native_unavailable",
                        format!("No {classifier} download exists for {}.", library.name),
                    )
                })?;
            let relative = native.path.as_ref().ok_or_else(|| {
                AppError::new(
                    "invalid_library",
                    format!("Native library {} has no path.", library.name),
                )
            })?;
            let target = paths.libraries.join(relative);
            download::ensure(
                &client,
                &format!("native library {} ({classifier})", library.name),
                &native.url,
                &native.sha1,
                native.size,
                &target,
            )
            .await?;
            extract_native(&target, &natives_dir, library.extract.as_ref())?;
        }
    }

    let asset_index_path = paths
        .assets
        .join("indexes")
        .join(format!("{}.json", metadata.asset_index.id));
    download::ensure(
        &client,
        &format!("asset index {}", metadata.asset_index.id),
        &metadata.asset_index.url,
        &metadata.asset_index.sha1,
        metadata.asset_index.size,
        &asset_index_path,
    )
    .await?;
    let index: AssetIndex = serde_json::from_slice(&tokio::fs::read(asset_index_path).await?)?;
    prepare_assets(app, &client, paths, index).await?;

    let logging_config = if let Some(logging) = &metadata.logging {
        let target = paths
            .assets
            .join("log_configs")
            .join(&logging.client.file.id);
        download::ensure(
            &client,
            &format!("logging configuration {}", logging.client.file.id),
            &logging.client.file.url,
            &logging.client.file.sha1,
            logging.client.file.size,
            &target,
        )
        .await?;
        Some(target)
    } else {
        None
    };
    Ok(PreparedVersion {
        metadata,
        client_jar,
        classpath,
        natives_dir,
        logging_config,
    })
}

async fn prepare_assets(
    app: &AppHandle,
    client: &reqwest::Client,
    paths: &AppPaths,
    index: AssetIndex,
) -> Result<()> {
    let total = index.objects.len();
    let completed = Arc::new(AtomicUsize::new(0));
    let objects_dir = paths.assets.join("objects");
    let downloads = index.objects.into_values().map(|asset| {
        let client = client.clone();
        let app = app.clone();
        let completed = completed.clone();
        let hash = asset.hash;
        let prefix = hash.get(0..2).unwrap_or("").to_string();
        let path = objects_dir.join(&prefix).join(&hash);
        async move {
            download::ensure(
                &client,
                &format!("asset {hash}"),
                &format!("https://resources.download.minecraft.net/{prefix}/{}", hash),
                &hash,
                asset.size,
                &path,
            )
            .await?;
            let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
            if count == total || count % 100 == 0 {
                emit(
                    &app,
                    "downloading",
                    format!("Preparing assets ({count}/{total})…"),
                    Some(0.2 + 0.75 * count as f64 / total.max(1) as f64),
                );
            }
            Ok::<(), AppError>(())
        }
    });
    stream::iter(downloads)
        .buffer_unordered(12)
        .try_collect::<Vec<_>>()
        .await?;
    Ok(())
}

fn extract_native(
    archive_path: &Path,
    destination: &Path,
    rules: Option<&ExtractRules>,
) -> Result<()> {
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        if relative
            .to_string_lossy()
            .replace('\\', "/")
            .starts_with("META-INF/")
        {
            continue;
        }
        if rules.is_some_and(|rules| {
            rules.exclude.iter().any(|prefix| {
                relative
                    .to_string_lossy()
                    .replace('\\', "/")
                    .starts_with(prefix)
            })
        }) {
            continue;
        }
        if entry.is_dir() {
            continue;
        }
        let output = destination.join(relative);
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut target = File::create(output)?;
        io::copy(&mut entry, &mut target)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn native_artifact_matches_platform(name: &str) -> bool {
    name.contains(":natives-windows")
}

#[cfg(target_os = "linux")]
fn native_artifact_matches_platform(name: &str) -> bool {
    name.contains(":natives-linux")
}

#[cfg(target_os = "windows")]
fn native_template(library: &Library) -> Option<&str> {
    library.natives.as_ref()?.get("windows").map(String::as_str)
}

#[cfg(target_os = "linux")]
fn native_template(library: &Library) -> Option<&str> {
    library.natives.as_ref()?.get("linux").map(String::as_str)
}

#[cfg(test)]
mod platform_native_tests {
    use super::*;
    use std::collections::HashMap;

    #[cfg(target_os = "linux")]
    #[test]
    fn selects_linux_native_classifier_when_both_platforms_exist() {
        let library = Library {
            name: "org.lwjgl:lwjgl:3.3.3".into(),
            downloads: LibraryDownloads {
                artifact: None,
                classifiers: HashMap::new(),
            },
            natives: Some(HashMap::from([
                ("windows".into(), "lwjgl-windows-${arch}".into()),
                ("linux".into(), "lwjgl-linux-${arch}".into()),
            ])),
            rules: None,
            extract: None,
        };
        assert_eq!(native_template(&library), Some("lwjgl-linux-${arch}"));
        assert!(!native_artifact_matches_platform("org.lwjgl:lwjgl:natives-windows"));
        assert!(native_artifact_matches_platform("org.lwjgl:lwjgl:natives-linux"));
    }
}
