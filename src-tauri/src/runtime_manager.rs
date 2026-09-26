use crate::{
    error::{AppError, Result},
    java::{self, JavaInfo},
    minecraft::install,
    paths::AppPaths,
    settings::LauncherSettings,
};
use futures::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const ADOPTIUM_API: &str = "https://api.adoptium.net/v3";
const MAX_RUNTIME_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_RUNTIME_ENTRIES: usize = 100_000;

#[derive(Clone, Debug, Deserialize)]
struct AdoptiumRelease {
    binary: AdoptiumBinary,
}

#[derive(Clone, Debug, Deserialize)]
struct AdoptiumBinary {
    architecture: String,
    image_type: String,
    os: String,
    package: AdoptiumPackage,
}

#[derive(Clone, Debug, Deserialize)]
struct AdoptiumPackage {
    checksum: String,
    link: String,
    name: String,
    size: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct RuntimeAsset {
    url: String,
    checksum: String,
    filename: String,
    size: u64,
    image_type: String,
}

pub async fn select_or_prepare(
    app: &AppHandle,
    paths: &AppPaths,
    settings: &LauncherSettings,
    required_major: u32,
) -> Result<JavaInfo> {
    if !settings.automatic_java {
        return java::detect(paths, required_major, settings.manual_java_path.as_deref());
    }
    if let Ok(runtime) = java::detect(paths, required_major, None) {
        return Ok(runtime);
    }
    if !settings.automatic_java_management {
        return Err(missing_runtime(required_major));
    }

    cleanup_stale_staging(paths)?;
    install::emit(
        app,
        "preparing",
        format!("Preparing Java {required_major}…"),
        None,
    );
    let client = client()?;
    let asset = resolve_temurin_asset(&client, required_major).await?;
    let staging = paths
        .runtimes
        .join(format!(".staging-java-{required_major}-{}", Uuid::new_v4()));
    fs::create_dir_all(&staging)?;
    let _cleanup = StagingCleanup(staging.clone());
    let archive = staging.join(runtime_archive_filename());
    install::emit(
        app,
        "downloading",
        format!("Downloading Temurin Java {required_major}…"),
        Some(0.0),
    );
    download_verified(app, &client, &asset, &archive, required_major).await?;
    install::emit(
        app,
        "preparing",
        format!("Verifying Java {required_major} runtime…"),
        None,
    );
    verify_sha256(&archive, &asset.checksum)?;

    let extract_dir = staging.join("extract");
    let archive_for_extract = archive.clone();
    let extract_for_task = extract_dir.clone();
    install::emit(
        app,
        "preparing",
        format!("Installing Java {required_major} runtime…"),
        None,
    );
    tokio::task::spawn_blocking(move || extract_archive(&archive_for_extract, &extract_for_task))
        .await
        .map_err(|error| runtime_error("The Java installer task stopped unexpectedly.", error))??;

    let staged_java = find_java_executable(&extract_dir).ok_or_else(|| {
        AppError::new(
            "runtime_java_missing",
            format!("Flint couldn't prepare Java {required_major}."),
        )
        .with_detail(runtime_missing_binary_detail())
    })?;
    #[cfg(target_os = "linux")]
    set_linux_executable(&staged_java)?;
    let inspected = java::inspect(&staged_java).ok_or_else(|| {
        AppError::new(
            "runtime_verification_failed",
            format!("Flint couldn't prepare Java {required_major}."),
        )
        .with_detail("The extracted runtime did not start as a 64-bit Java installation.")
    })?;
    if inspected.major_version != required_major {
        return Err(AppError::new(
            "runtime_version_mismatch",
            format!("Flint couldn't prepare Java {required_major}."),
        )
        .with_detail(format!(
            "Temurin returned Java {} instead of Java {required_major}.",
            inspected.major_version
        )));
    }
    let runtime_root = staged_java.parent().and_then(Path::parent).ok_or_else(|| {
        AppError::new(
            "runtime_layout_invalid",
            "The Java archive layout was invalid.",
        )
    })?;
    let java_relative = staged_java
        .strip_prefix(runtime_root)
        .map_err(|error| runtime_error("The Java archive layout was invalid.", error))?
        .to_path_buf();
    let target = runtime_target(paths, required_major);
    promote_runtime(paths, runtime_root, &target)?;
    let final_java = target.join(java_relative);
    let runtime = java::inspect(&final_java).ok_or_else(|| {
        AppError::new(
            "runtime_promotion_failed",
            format!("Flint couldn't prepare Java {required_major}."),
        )
        .with_detail("The managed runtime could not be verified after installation.")
    })?;
    install::emit(
        app,
        "preparing",
        format!("Java {required_major} ready."),
        None,
    );
    tracing::info!(
        major = required_major,
        image_type = asset.image_type,
        package = asset.filename,
        path = %runtime.path.display(),
        "installed Flint-managed Temurin runtime"
    );
    Ok(runtime)
}

fn missing_runtime(required_major: u32) -> AppError {
    AppError::new(
        "java_not_found",
        format!(
            "Flint couldn't prepare Java {required_major}. Retry, enable managed runtimes, or choose Java manually in Settings."
        ),
    )
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!(
            "Flint/",
            env!("CARGO_PKG_VERSION"),
            " (runtime manager)"
        ))
        .https_only(true)
        .build()
        .map_err(|error| runtime_error("Flint couldn't initialize Java downloads.", error))
}

async fn resolve_temurin_asset(client: &reqwest::Client, major: u32) -> Result<RuntimeAsset> {
    for image_type in ["jre", "jdk"] {
        let response = client
            .get(format!("{ADOPTIUM_API}/assets/latest/{major}/hotspot"))
            .query(&[
                ("architecture", "x64"),
                ("image_type", image_type),
                ("os", target_os_name()),
                ("vendor", "eclipse"),
            ])
            .send()
            .await
            .map_err(|error| runtime_error("Flint couldn't find a Java download.", error))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        let releases: Vec<AdoptiumRelease> = response
            .error_for_status()
            .map_err(|error| runtime_error("Flint couldn't find a Java download.", error))?
            .json()
            .await
            .map_err(|error| {
                runtime_error("The Java provider returned invalid metadata.", error)
            })?;
        if let Some(asset) = asset_from_releases(releases) {
            return Ok(asset);
        }
    }
    Err(AppError::new(
        "runtime_unavailable",
        format!("Flint couldn't prepare Java {major}."),
    )
    .with_detail(format!(
        "Eclipse Adoptium did not offer a {} x64 Temurin runtime.",
        target_os_name()
    )))
}

fn asset_from_releases(releases: Vec<AdoptiumRelease>) -> Option<RuntimeAsset> {
    releases.into_iter().find_map(|release| {
        let binary = release.binary;
        let package = binary.package;
        let secure = reqwest::Url::parse(&package.link)
            .ok()
            .is_some_and(|url| url.scheme() == "https");
        let checksum_valid = package.checksum.len() == 64
            && package
                .checksum
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit());
        (binary.architecture == "x64"
            && binary.os == target_os_name()
            && matches!(binary.image_type.as_str(), "jre" | "jdk")
            && ((cfg!(target_os = "windows") && package.name.ends_with(".zip"))
                || (cfg!(target_os = "linux") && package.name.ends_with(".tar.gz")))
            && secure
            && checksum_valid
            && package.size > 0
            && package.size <= MAX_RUNTIME_BYTES)
            .then(|| RuntimeAsset {
                url: package.link,
                checksum: package.checksum,
                filename: package.name,
                size: package.size,
                image_type: binary.image_type,
            })
    })
}

async fn download_verified(
    app: &AppHandle,
    client: &reqwest::Client,
    asset: &RuntimeAsset,
    path: &Path,
    major: u32,
) -> Result<()> {
    let response = client
        .get(&asset.url)
        .send()
        .await
        .map_err(|error| runtime_error("The Java runtime download failed.", error))?
        .error_for_status()
        .map_err(|error| runtime_error("The Java runtime download failed.", error))?;
    if response.url().scheme() != "https" {
        return Err(AppError::new(
            "runtime_insecure_redirect",
            format!("Flint couldn't prepare Java {major}."),
        )
        .with_detail("The runtime provider redirected to a non-HTTPS URL."));
    }
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(path).await?;
    let mut received = 0_u64;
    let mut digest = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|error| runtime_error("The Java runtime download was interrupted.", error))?;
        received = received.saturating_add(chunk.len() as u64);
        if received > MAX_RUNTIME_BYTES || received > asset.size {
            return Err(AppError::new(
                "runtime_download_size_mismatch",
                format!("Flint couldn't prepare Java {major}."),
            )
            .with_detail("The runtime download exceeded the provider-declared size."));
        }
        digest.update(&chunk);
        file.write_all(&chunk).await?;
        install::emit(
            app,
            "downloading",
            format!(
                "Downloading Java {major}… {} / {} MB",
                received / 1_048_576,
                asset.size / 1_048_576
            ),
            Some(received as f64 / asset.size as f64),
        );
    }
    file.flush().await?;
    if received != asset.size {
        return Err(AppError::new(
            "runtime_download_incomplete",
            format!("Flint couldn't prepare Java {major}."),
        )
        .with_detail(format!(
            "Expected {} bytes but received {received}.",
            asset.size
        )));
    }
    let actual = hex::encode(digest.finalize());
    if !actual.eq_ignore_ascii_case(&asset.checksum) {
        return Err(checksum_error(&asset.checksum, &actual));
    }
    Ok(())
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let actual = hex::encode(digest.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(checksum_error(expected, &actual));
    }
    Ok(())
}

fn checksum_error(expected: &str, actual: &str) -> AppError {
    AppError::new(
        "runtime_checksum_mismatch",
        "Flint rejected a Java runtime that failed integrity verification.",
    )
    .with_detail(format!("Expected SHA-256 {expected}, received {actual}."))
}

#[cfg(windows)]
fn extract_archive(archive_path: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    let file = fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| runtime_error("Flint rejected a corrupt Java runtime archive.", error))?;
    if archive.len() > MAX_RUNTIME_ENTRIES {
        return Err(AppError::new(
            "runtime_archive_too_large",
            "Flint rejected an unexpectedly large Java runtime archive.",
        ));
    }
    let mut extracted = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            runtime_error("Flint rejected a corrupt Java runtime archive.", error)
        })?;
        let relative = entry.enclosed_name().ok_or_else(|| {
            AppError::new(
                "runtime_archive_traversal",
                "Flint rejected an unsafe Java runtime archive.",
            )
        })?;
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(AppError::new(
                "runtime_archive_traversal",
                "Flint rejected an unsafe Java runtime archive.",
            ));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(AppError::new(
                "runtime_archive_symlink",
                "Flint rejected a Java runtime archive containing links.",
            ));
        }
        extracted = extracted.saturating_add(entry.size());
        if extracted > MAX_RUNTIME_BYTES {
            return Err(AppError::new(
                "runtime_archive_too_large",
                "Flint rejected an unexpectedly large Java runtime archive.",
            ));
        }
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut target = fs::File::create(output)?;
        std::io::copy(&mut entry, &mut target)?;
        target.flush()?;
    }
    Ok(())
}

#[cfg(windows)]
fn find_java_executable(root: &Path) -> Option<PathBuf> {
    let direct = root.join("bin/java.exe");
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(root)
        .ok()?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        .find_map(|entry| {
            let candidate = entry.path().join("bin/java.exe");
            candidate.is_file().then_some(candidate)
        })
}

#[cfg(target_os = "linux")]
fn extract_archive(archive_path: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    let file = fs::File::open(archive_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut extracted = 0_u64;
    let entries = archive
        .entries()
        .map_err(|error| runtime_error("Flint rejected a corrupt Java runtime archive.", error))?;
    for (index, item) in entries.enumerate() {
        if index >= MAX_RUNTIME_ENTRIES {
            return Err(AppError::new(
                "runtime_archive_too_large",
                "Flint rejected an unexpectedly large Java runtime archive.",
            ));
        }
        let mut entry = item.map_err(|error| {
            runtime_error("Flint rejected a corrupt Java runtime archive.", error)
        })?;
        let entry_type = entry.header().entry_type();
        // Real Temurin/OpenJDK Linux tarballs legitimately contain symlinks
        // (mainly under legal/, where shared license text is symlinked
        // instead of duplicated per module). Rejecting every symlink broke
        // every real download. Hard links are rare in these archives and
        // harder to validate safely, so those stay rejected.
        if entry_type.is_hard_link() {
            return Err(AppError::new(
                "runtime_archive_symlink",
                "Flint rejected a Java runtime archive containing hard links.",
            ));
        }
        let relative = entry
            .path()
            .map_err(|error| {
                runtime_error("Flint rejected a corrupt Java runtime archive.", error)
            })?
            .into_owned();
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(AppError::new(
                "runtime_archive_traversal",
                "Flint rejected an unsafe Java runtime archive.",
            ));
        }
        let size = entry.size();
        extracted = extracted.saturating_add(size);
        if extracted > MAX_RUNTIME_BYTES {
            return Err(AppError::new(
                "runtime_archive_too_large",
                "Flint rejected an unexpectedly large Java runtime archive.",
            ));
        }
        let output = destination.join(&relative);
        if entry_type.is_dir() {
            fs::create_dir_all(output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        if entry_type.is_symlink() {
            let Some(link_target) = entry.link_name().map_err(|error| {
                runtime_error("Flint rejected a corrupt Java runtime archive.", error)
            })?
            else {
                return Err(AppError::new(
                    "runtime_archive_traversal",
                    "Flint rejected a Java runtime archive with an unresolvable link.",
                ));
            };
            if link_target.is_absolute() {
                return Err(AppError::new(
                    "runtime_archive_traversal",
                    "Flint rejected a Java runtime archive with an absolute link target.",
                ));
            }
            // Resolve the link lexically, relative to its own location inside
            // the archive, and make sure it can never climb above the
            // destination root before it descends back in.
            let mut resolved: Vec<std::ffi::OsString> = relative
                .parent()
                .into_iter()
                .flat_map(|parent| parent.components())
                .map(|component| component.as_os_str().to_owned())
                .collect();
            for component in link_target.components() {
                match component {
                    Component::ParentDir => {
                        if resolved.pop().is_none() {
                            return Err(AppError::new(
                                "runtime_archive_traversal",
                                "Flint rejected a Java runtime archive with a link outside the archive.",
                            ));
                        }
                    }
                    Component::Normal(part) => resolved.push(part.to_owned()),
                    Component::CurDir => {}
                    _ => {
                        return Err(AppError::new(
                            "runtime_archive_traversal",
                            "Flint rejected a Java runtime archive with an unsafe link target.",
                        ));
                    }
                }
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&link_target, &output)?;
            continue;
        }
        let mode = entry.header().mode().map_err(|error| {
            runtime_error("Flint rejected a corrupt Java runtime archive.", error)
        })?;
        let mut target = fs::File::create(&output)?;
        std::io::copy(&mut entry, &mut target)?;
        target.flush()?;
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            // Preserve executable bits needed by Java helpers such as
            // lib/jspawnhelper, while stripping group/world write access.
            fs::set_permissions(
                &output,
                fs::Permissions::from_mode(mode & 0o755),
            )?;
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn find_java_executable(root: &Path) -> Option<PathBuf> {
    let direct = root.join("bin/java");
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(root)
        .ok()?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        .find_map(|entry| {
            let candidate = entry.path().join("bin/java");
            candidate.is_file().then_some(candidate)
        })
}

#[cfg(target_os = "linux")]
fn set_linux_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o100);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(windows)]
fn runtime_archive_filename() -> &'static str {
    "runtime.zip"
}

#[cfg(target_os = "linux")]
fn runtime_archive_filename() -> &'static str {
    "runtime.tar.gz"
}

#[cfg(windows)]
fn runtime_missing_binary_detail() -> &'static str {
    "The verified Temurin archive did not contain bin/java.exe."
}

#[cfg(target_os = "linux")]
fn runtime_missing_binary_detail() -> &'static str {
    "The verified Temurin archive did not contain bin/java."
}

#[cfg(windows)]
fn target_os_name() -> &'static str {
    "windows"
}

#[cfg(target_os = "linux")]
fn target_os_name() -> &'static str {
    "linux"
}

fn runtime_target(paths: &AppPaths, major: u32) -> PathBuf {
    paths.runtimes.join(format!("java-{major}"))
}

fn promote_runtime(paths: &AppPaths, staged: &Path, target: &Path) -> Result<()> {
    if staged == target
        || !staged.starts_with(&paths.runtimes)
        || !target.starts_with(&paths.runtimes)
    {
        return Err(AppError::new(
            "runtime_path_invalid",
            "Flint refused an unsafe managed-runtime path.",
        ));
    }
    let backup = paths
        .runtimes
        .join(format!(".backup-java-{}", Uuid::new_v4()));
    let had_target = target.exists();
    if had_target {
        fs::rename(target, &backup)?;
    }
    if let Err(error) = fs::rename(staged, target) {
        if had_target {
            let _ = fs::rename(&backup, target);
        }
        return Err(runtime_error(
            "Flint couldn't install the managed Java runtime.",
            error,
        ));
    }
    if had_target {
        fs::remove_dir_all(backup)?;
    }
    Ok(())
}

fn cleanup_stale_staging(paths: &AppPaths) -> Result<()> {
    fs::create_dir_all(&paths.runtimes)?;
    for entry in fs::read_dir(&paths.runtimes)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".staging-java-")
            && entry.path().is_dir()
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

fn runtime_error(message: &'static str, error: impl std::fmt::Display) -> AppError {
    AppError::new("runtime_error", message).with_detail(error.to_string())
}

struct StagingCleanup(PathBuf);

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[cfg(windows)]
    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        for (name, contents) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn provider_metadata_requires_windows_x64_https_and_checksum() {
        let release = AdoptiumRelease {
            binary: AdoptiumBinary {
                architecture: "x64".into(),
                image_type: "jre".into(),
                os: "windows".into(),
                package: AdoptiumPackage {
                    checksum: "a".repeat(64),
                    link: "https://example.test/runtime.zip".into(),
                    name: "runtime.zip".into(),
                    size: 42,
                },
            },
        };
        assert!(asset_from_releases(vec![release]).is_some());
        let insecure = AdoptiumRelease {
            binary: AdoptiumBinary {
                architecture: "x64".into(),
                image_type: "jdk".into(),
                os: "windows".into(),
                package: AdoptiumPackage {
                    checksum: "a".repeat(64),
                    link: "http://example.test/runtime.zip".into(),
                    name: "runtime.zip".into(),
                    size: 42,
                },
            },
        };
        assert!(asset_from_releases(vec![insecure]).is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn provider_metadata_requires_linux_x64_tarball() {
        let release = AdoptiumRelease {
            binary: AdoptiumBinary {
                architecture: "x64".into(),
                image_type: "jre".into(),
                os: "linux".into(),
                package: AdoptiumPackage {
                    checksum: "a".repeat(64),
                    link: "https://example.test/runtime.tar.gz".into(),
                    name: "runtime.tar.gz".into(),
                    size: 42,
                },
            },
        };
        assert!(asset_from_releases(vec![release]).is_some());
    }

    #[test]
    fn checksum_mismatch_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("runtime.zip");
        fs::write(&archive, b"runtime").unwrap();
        let error = verify_sha256(&archive, &"0".repeat(64)).unwrap_err();
        assert_eq!(error.code, "runtime_checksum_mismatch");
    }

    #[test]
    fn corrupt_archive_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("runtime.zip");
        fs::write(&archive, b"not a zip").unwrap();
        assert!(extract_archive(&archive, &temp.path().join("out")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn archive_traversal_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("runtime.zip");
        write_zip(&archive, &[("../outside.exe", b"bad")]);
        let error = extract_archive(&archive, &temp.path().join("out")).unwrap_err();
        assert_eq!(error.code, "runtime_archive_traversal");
        assert!(!temp.path().join("outside.exe").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tar_archive_traversal_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("runtime.tar.gz");
        let file = fs::File::create(&archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.as_mut_bytes()[..11].copy_from_slice(b"../outside\0");
        header.set_size(3);
        header.set_cksum();
        builder.append(&header, &b"bad"[..]).unwrap();
        builder.into_inner().unwrap().finish().unwrap();
        let error = extract_archive(&archive, &temp.path().join("out")).unwrap_err();
        assert_eq!(error.code, "runtime_archive_traversal");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tar_extraction_preserves_runtime_helper_executable_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("runtime.tar.gz");
        let file = fs::File::create(&archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_path("jdk/lib/jspawnhelper").unwrap();
        header.set_size(4);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append(&header, &b"exec"[..]).unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        let destination = temp.path().join("out");
        extract_archive(&archive, &destination).unwrap();
        let helper = destination.join("jdk/lib/jspawnhelper");
        let mode = fs::metadata(helper).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn stale_partial_installation_is_removed_only_from_managed_storage() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(temp.path().join("flint"));
        paths.ensure().unwrap();
        let partial = paths.runtimes.join(".staging-java-21-interrupted");
        fs::create_dir_all(&partial).unwrap();
        fs::write(partial.join("partial"), b"partial").unwrap();
        let outside = temp.path().join("system-java.exe");
        fs::write(&outside, b"untouched").unwrap();
        cleanup_stale_staging(&paths).unwrap();
        assert!(!partial.exists());
        assert_eq!(fs::read(outside).unwrap(), b"untouched");
    }

    #[test]
    fn multiple_managed_java_majors_have_distinct_targets() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(temp.path());
        assert_ne!(runtime_target(&paths, 8), runtime_target(&paths, 21));
        assert_ne!(runtime_target(&paths, 21), runtime_target(&paths, 25));
    }

    #[tokio::test]
    async fn live_temurin_metadata_when_enabled() {
        if std::env::var_os("FLINT_LIVE_TEST").is_none() {
            return;
        }
        let asset = resolve_temurin_asset(&client().unwrap(), 21).await.unwrap();
        assert!(asset.url.starts_with("https://"));
        assert_eq!(asset.checksum.len(), 64);
        assert!(asset.size > 0);
    }
}
