use serde::Deserialize;
use std::collections::HashMap;

pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Clone, Debug, Deserialize)]
pub struct VersionManifest {
    pub versions: Vec<VersionReference>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionReference {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub release_time: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionMetadata {
    pub id: String,
    pub main_class: String,
    pub assets: String,
    pub asset_index: AssetIndexReference,
    pub downloads: VersionDownloads,
    pub libraries: Vec<Library>,
    pub arguments: Option<Arguments>,
    pub minecraft_arguments: Option<String>,
    pub java_version: Option<JavaVersion>,
    pub logging: Option<Logging>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub major_version: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AssetIndexReference {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VersionDownloads {
    pub client: Download,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Download {
    pub path: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Library {
    pub name: String,
    pub downloads: LibraryDownloads,
    pub natives: Option<HashMap<String, String>>,
    pub rules: Option<Vec<Rule>>,
    pub extract: Option<ExtractRules>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<Download>,
    #[serde(default)]
    pub classifiers: HashMap<String, Download>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExtractRules {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgumentValue,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Clone, Debug, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    pub os: Option<OsRule>,
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Logging {
    pub client: LoggingClient,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: LoggingFile,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

pub fn rules_allow(rules: Option<&[Rule]>) -> bool {
    let Some(rules) = rules else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule) {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

fn rule_matches(rule: &Rule) -> bool {
    if let Some(os) = &rule.os {
        if os.name.as_deref().is_some_and(|name| {
            if cfg!(target_os = "windows") {
                name != "windows"
            } else if cfg!(target_os = "linux") {
                name != "linux"
            } else {
                true
            }
        }) {
            return false;
        }
        let arch = if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86"
        };
        if os.arch.as_deref().is_some_and(|required| required != arch) {
            return false;
        }
        if os.version.is_some() {
            // Version-constrained legacy rules are uncommon; an unknown platform version must not match.
            return false;
        }
    }
    if let Some(features) = &rule.features {
        // Milestone 1 enables no optional launcher features (demo, quick play, custom resolution).
        if features.values().any(|required| *required) {
            return false;
        }
    }
    true
}

pub fn resolved_argument(argument: &Argument) -> Vec<String> {
    match argument {
        Argument::Plain(value) => vec![value.clone()],
        Argument::Conditional { rules, value } if rules_allow(Some(rules)) => match value {
            ArgumentValue::One(value) => vec![value.clone()],
            ArgumentValue::Many(values) => values.clone(),
        },
        Argument::Conditional { .. } => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disallow_rule_overrides_allow_rule() {
        let rules = vec![
            Rule {
                action: RuleAction::Allow,
                os: None,
                features: None,
            },
            Rule {
                action: RuleAction::Disallow,
                os: Some(OsRule {
                    name: Some("windows".into()),
                    arch: None,
                    version: None,
                }),
                features: None,
            },
        ];
        assert!(!rules_allow(Some(&rules)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_rules_match_linux_and_reject_windows() {
        let linux = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("linux".into()),
                arch: None,
                version: None,
            }),
            features: None,
        };
        let windows = Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("windows".into()),
                arch: None,
                version: None,
            }),
            features: None,
        };
        assert!(rules_allow(Some(&[linux])));
        assert!(!rules_allow(Some(&[windows])));
    }

    #[test]
    fn parses_configured_live_version_metadata() {
        let Ok(path) = std::env::var("FLINT_TEST_VERSION_METADATA") else {
            return;
        };
        let bytes = std::fs::read(path).expect("configured metadata fixture was not readable");
        let metadata: VersionMetadata = serde_json::from_slice(&bytes)
            .expect("live Mojang metadata did not match Flint's model");
        assert!(!metadata.id.is_empty());
        assert!(metadata
            .java_version
            .as_ref()
            .is_some_and(|java| java.major_version >= 8));
        assert!(!metadata.libraries.is_empty());
        assert!(metadata.arguments.is_some());
    }
}
