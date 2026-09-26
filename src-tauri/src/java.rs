use crate::{
    error::{AppError, Result},
    paths::AppPaths,
};
use regex::Regex;
use serde::Serialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInfo {
    pub path: PathBuf,
    pub major_version: u32,
    pub description: String,
    pub architecture: String,
    pub source: JavaSource,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JavaSource {
    Managed,
    System,
    Manual,
}

pub fn list(paths: &AppPaths) -> Vec<JavaInfo> {
    let mut candidates = managed_candidates(paths)
        .into_iter()
        .map(|path| (path, JavaSource::Managed))
        .collect::<Vec<_>>();
    #[cfg(windows)]
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        candidates.push((PathBuf::from(home).join("bin/java.exe"), JavaSource::System));
    }
    #[cfg(windows)]
    if let Ok(output) = crate::process_command::std_command("where.exe")
        .arg("java.exe")
        .output()
    {
        candidates.extend(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|path| (PathBuf::from(path), JavaSource::System)),
        );
    }
    #[cfg(windows)]
    for root in [
        r"C:\Program Files\Eclipse Adoptium",
        r"C:\Program Files\Java",
        r"C:\Program Files\Microsoft",
        r"C:\Program Files\Amazon Corretto",
    ] {
        candidates.extend(
            java_children(Path::new(root))
                .into_iter()
                .map(|path| (path, JavaSource::System)),
        );
    }
    #[cfg(windows)]
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        candidates.extend(
            java_children(&PathBuf::from(local_app_data).join(r"Programs\Eclipse Adoptium"))
                .into_iter()
                .map(|path| (path, JavaSource::System)),
        );
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("JAVA_HOME") {
            candidates.push((PathBuf::from(home).join("bin/java"), JavaSource::System));
        }
        if let Some(path) = std::env::var_os("PATH") {
            candidates.extend(std::env::split_paths(&path).map(|directory| {
                (directory.join("java"), JavaSource::System)
            }));
        }
        for root in linux_java_roots() {
            candidates.extend(
                linux_java_candidates(&root)
                    .into_iter()
                    .map(|path| (path, JavaSource::System)),
            );
        }
    }
    let mut seen_paths = HashSet::new();
    let mut seen_installations = HashSet::new();
    let mut detected = Vec::new();
    for (candidate, source) in candidates {
        let path_key = normalized_path(&candidate);
        if !is_candidate_executable(&candidate) || !seen_paths.insert(path_key) {
            continue;
        }
        if let Some((info, installation_key)) = inspect_with_identity(&candidate, source) {
            if seen_installations.insert(installation_key) {
                detected.push(info);
            }
        }
    }
    detected.sort_by_key(|info| std::cmp::Reverse(info.major_version));
    detected
}

pub fn detect(
    paths: &AppPaths,
    required_major: u32,
    manual_path: Option<&Path>,
) -> Result<JavaInfo> {
    let manual = manual_path.and_then(|path| inspect_as(path, JavaSource::Manual));
    let selected = select_detected(
        required_major,
        manual_path.is_some(),
        manual,
        list(paths),
    )
    .ok_or_else(|| {
        AppError::new(
            "java_not_found",
            format!(
                "This Minecraft version requires a 64-bit Java {required_major} runtime. Install Temurin or Microsoft OpenJDK {required_major}, or choose a compatible executable in Settings."
            ),
        )
    })?;
    tracing::info!(
        path = %selected.path.display(),
        major_version = selected.major_version,
        "detected compatible Java runtime"
    );
    Ok(selected)
}

#[cfg(windows)]
fn java_children(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("bin/java.exe"))
        .collect()
}

#[cfg(target_os = "linux")]
fn java_children(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("bin/java"))
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_java_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/usr/lib/jvm")];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".sdkman/candidates/java"));
        roots.push(home.join(".var/app"));
    }
    roots.extend([
        PathBuf::from("/var/lib/flatpak/app"),
        PathBuf::from("/snap"),
        PathBuf::from("/opt"),
    ]);
    roots
}

#[cfg(target_os = "linux")]
fn linux_java_candidates(root: &Path) -> Vec<PathBuf> {
    let mut candidates = java_children(root);
    let mut pending = vec![(root.to_path_buf(), 0_u8)];
    while let Some((directory, depth)) = pending.pop() {
        if depth >= 5 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("bin/java").is_file() {
                candidates.push(path.join("bin/java"));
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push((path, depth + 1));
            }
        }
    }
    candidates
}

#[cfg(windows)]
fn is_candidate_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(target_os = "linux")]
fn is_candidate_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

pub fn inspect(path: &Path) -> Option<JavaInfo> {
    inspect_as(path, JavaSource::Managed)
}

fn inspect_as(path: &Path, source: JavaSource) -> Option<JavaInfo> {
    inspect_with_identity(path, source).map(|(info, _)| info)
}

fn inspect_with_identity(path: &Path, source: JavaSource) -> Option<(JavaInfo, String)> {
    let output = crate::process_command::std_command(path)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let regex = Regex::new(r#"version "(\d+)(?:\.(\d+))?"#).ok()?;
    let captures = regex.captures(&text)?;
    let first: u32 = captures.get(1)?.as_str().parse().ok()?;
    let major = if first == 1 {
        captures.get(2)?.as_str().parse().ok()?
    } else {
        first
    };
    let architecture = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("os.arch ="))
        .map(str::trim)
        .and_then(normalized_architecture)?;
    if architecture != current_architecture() {
        return None;
    }
    let description = text
        .lines()
        .next()
        .unwrap_or("Java runtime")
        .trim()
        .to_string();
    let installation_key = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("java.home ="))
        .map(|home| normalized_path(Path::new(home.trim())))
        .unwrap_or_else(|| normalized_path(path));
    Some((
        JavaInfo {
            path: path.to_path_buf(),
            major_version: major,
            description,
            architecture: architecture.into(),
            source,
        },
        installation_key,
    ))
}

fn select_detected(
    required_major: u32,
    manual_requested: bool,
    manual: Option<JavaInfo>,
    automatic: Vec<JavaInfo>,
) -> Option<JavaInfo> {
    if manual_requested {
        return manual.filter(|runtime| runtime.major_version == required_major);
    }
    automatic
        .into_iter()
        .find(|runtime| runtime.major_version == required_major)
}

fn normalized_architecture(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "amd64" | "x86_64" => Some("x64"),
        "aarch64" | "arm64" => Some("arm64"),
        _ => None,
    }
}

fn current_architecture() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unsupported"
    }
}

fn managed_candidates(paths: &AppPaths) -> Vec<PathBuf> {
    java_children(&paths.runtimes)
        .into_iter()
        .chain(
            std::fs::read_dir(&paths.runtimes)
                .into_iter()
                .flatten()
                .flatten()
                .flat_map(|entry| java_children(&entry.path())),
        )
        .filter(|path| path.is_file())
        .collect()
}

fn normalized_path(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        detect, managed_candidates, normalized_architecture, normalized_path, select_detected,
        JavaInfo, JavaSource,
    };
    use crate::paths::AppPaths;
    use std::path::{Path, PathBuf};

    #[test]
    fn java_version_regex_handles_modern_versions() {
        let regex = regex::Regex::new(r#"version "(\d+)(?:\.(\d+))?"#).unwrap();
        let captures = regex
            .captures(r#"openjdk version "21.0.8" 2025-07-15"#)
            .unwrap();
        assert_eq!(captures.get(1).unwrap().as_str(), "21");
    }

    #[test]
    fn detects_installed_runtime_when_requested() {
        let Ok(expected) = std::env::var("FLINT_TEST_JAVA_MAJOR") else {
            return;
        };
        let expected: u32 = expected
            .parse()
            .expect("FLINT_TEST_JAVA_MAJOR must be numeric");
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(temp.path());
        let detected = detect(&paths, expected, None)
            .expect("requested installed Java runtime was not detected");
        assert_eq!(detected.major_version, expected);
        assert!(detected.path.is_file());
        assert_eq!(detected.source, JavaSource::System);
    }

    #[test]
    fn equivalent_windows_paths_share_a_deduplication_key() {
        assert_eq!(
            normalized_path(Path::new(r"C:\Java\Temurin\")),
            normalized_path(Path::new(r"c:\java\temurin"))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn equivalent_linux_paths_share_a_deduplication_key() {
        let temp = tempfile::tempdir().unwrap();
        let java_home = temp.path().join("java");
        std::fs::create_dir_all(&java_home).unwrap();
        assert_eq!(
            normalized_path(&java_home),
            normalized_path(&java_home.join("."))
        );
    }

    #[test]
    fn architecture_is_detected_without_mislabeling_arm64() {
        assert_eq!(normalized_architecture("amd64"), Some("x64"));
        assert_eq!(normalized_architecture("x86_64"), Some("x64"));
        assert_eq!(normalized_architecture("aarch64"), Some("arm64"));
        assert_eq!(normalized_architecture("x86"), None);
    }

    fn info(major: u32, source: JavaSource) -> JavaInfo {
        JavaInfo {
            path: PathBuf::from(format!(r"C:\Java\{major}\bin\java.exe")),
            major_version: major,
            description: "Test Java".into(),
            architecture: "x64".into(),
            source,
        }
    }

    #[test]
    fn manual_override_has_strict_precedence() {
        let selected = select_detected(
            21,
            true,
            Some(info(17, JavaSource::Manual)),
            vec![info(21, JavaSource::System)],
        );
        assert!(selected.is_none());
        let selected = select_detected(
            21,
            true,
            Some(info(21, JavaSource::Manual)),
            vec![info(21, JavaSource::Managed)],
        )
        .unwrap();
        assert_eq!(selected.source, JavaSource::Manual);
    }

    #[test]
    fn automatic_selection_reuses_a_compatible_major() {
        let selected = select_detected(
            21,
            false,
            None,
            vec![
                info(25, JavaSource::System),
                info(21, JavaSource::Managed),
                info(17, JavaSource::System),
            ],
        )
        .unwrap();
        assert_eq!(selected.major_version, 21);
        assert_eq!(selected.source, JavaSource::Managed);
    }

    #[test]
    fn managed_runtime_candidates_are_isolated_by_major() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(temp.path());
        for major in [17, 21, 25] {
            let bin = paths.runtimes.join(format!("java-{major}/bin"));
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("java.exe"), b"fixture").unwrap();
        }
        let candidates = managed_candidates(&paths);
        assert_eq!(candidates.len(), 3);
        assert!(candidates
            .iter()
            .all(|path| path.starts_with(&paths.runtimes)));
    }
}
