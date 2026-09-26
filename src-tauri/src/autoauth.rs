use crate::{
    error::{AppError, Result},
    paths::AppPaths,
    profiles,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use uuid::Uuid;
use zeroize::Zeroize;

const PROTOCOL_VERSION: u8 = 1;
const SERVICE: &str = "Flint AutoAuth";

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoAuthMode {
    #[default]
    Disabled,
    Login,
    Register,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredRule {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<AutoAuthMode>,
    #[serde(default, skip_serializing)]
    enabled: Option<bool>,
    server_address: String,
    login_template: String,
    registration_template: String,
    credential_ref: String,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredConfig {
    #[serde(default = "protocol_version")]
    protocol_version: u8,
    #[serde(default)]
    rules: Vec<StoredRule>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoAuthRule {
    pub id: String,
    pub mode: AutoAuthMode,
    pub server_address: String,
    pub login_template: String,
    pub registration_template: String,
    pub has_credential: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoAuthInput {
    pub id: Option<String>,
    pub mode: AutoAuthMode,
    pub server_address: String,
    pub login_template: String,
    pub registration_template: String,
    pub password: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeRequest {
    token: String,
    server: String,
    session_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeResponse {
    command: Option<String>,
    error: Option<&'static str>,
}

pub struct AutoAuthSession {
    endpoint: String,
    token: String,
    shutdown: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

fn protocol_version() -> u8 {
    PROTOCOL_VERSION
}

impl StoredRule {
    fn effective_mode(&self) -> AutoAuthMode {
        self.mode.unwrap_or(if self.enabled.unwrap_or(false) {
            AutoAuthMode::Login
        } else {
            AutoAuthMode::Disabled
        })
    }
}

fn file(paths: &AppPaths, profile_id: &str) -> PathBuf {
    paths
        .instance(profile_id)
        .join("flint")
        .join("autoauth-v1.json")
}

pub fn list(paths: &AppPaths, profile_id: &str) -> Result<Vec<AutoAuthRule>> {
    profiles::find(paths, profile_id)?;
    Ok(load(paths, profile_id)?
        .rules
        .into_iter()
        .map(|rule| {
            let mode = rule.effective_mode();
            AutoAuthRule {
                id: rule.id,
                mode,
                server_address: rule.server_address,
                login_template: rule.login_template,
                registration_template: rule.registration_template,
                has_credential: true,
            }
        })
        .collect())
}

pub fn save(paths: &AppPaths, profile_id: &str, input: AutoAuthInput) -> Result<Vec<AutoAuthRule>> {
    profiles::find(paths, profile_id)?;
    let server = validate_server(&input.server_address)?;
    validate_template(&input.login_template, false)?;
    validate_template(&input.registration_template, true)?;
    let mut config = load(paths, profile_id)?;
    let existing_index = input
        .id
        .as_ref()
        .and_then(|id| config.rules.iter().position(|rule| &rule.id == id));
    let credential_ref = existing_index
        .map(|index| config.rules[index].credential_ref.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if existing_index.is_none() && input.password.as_deref().is_none_or(str::is_empty) {
        return Err(AppError::new(
            "autoauth_password_required",
            "Enter the server password before saving AutoAuth.",
        ));
    }
    if let Some(password) = input.password.as_deref() {
        if password.is_empty() || password.len() > 512 || password.contains(['\r', '\n', '\0']) {
            return Err(AppError::new(
                "invalid_autoauth_password",
                "The password is empty or too long.",
            ));
        }
        credential(&credential_ref)?
            .set_password(password)
            .map_err(credential_error)?;
    }
    let rule = StoredRule {
        id: input.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        mode: Some(input.mode),
        enabled: None,
        server_address: server,
        login_template: input.login_template,
        registration_template: input.registration_template,
        credential_ref,
    };
    if let Some(index) = existing_index {
        config.rules[index] = rule;
    } else {
        config.rules.push(rule);
    }
    write(paths, profile_id, &config)?;
    list(paths, profile_id)
}

pub fn remove(paths: &AppPaths, profile_id: &str, id: &str) -> Result<Vec<AutoAuthRule>> {
    profiles::find(paths, profile_id)?;
    let mut config = load(paths, profile_id)?;
    let index = config
        .rules
        .iter()
        .position(|rule| rule.id == id)
        .ok_or_else(|| {
            AppError::new(
                "autoauth_not_found",
                "That AutoAuth entry no longer exists.",
            )
        })?;
    let removed = config.rules.remove(index);
    let entry = credential(&removed.credential_ref)?;
    let mut recovery_secret = match entry.get_password() {
        Ok(secret) => Some(secret),
        Err(keyring::Error::NoEntry) => None,
        Err(error) => return Err(credential_error(error)),
    };
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(credential_error(error)),
    }
    if let Err(error) = write(paths, profile_id, &config) {
        if let Some(secret) = &recovery_secret {
            let _ = entry.set_password(secret);
        }
        recovery_secret.zeroize();
        return Err(error);
    }
    recovery_secret.zeroize();
    list(paths, profile_id)
}

pub fn start_session(paths: &AppPaths, profile_id: &str) -> Result<Option<AutoAuthSession>> {
    let rules: Vec<_> = load(paths, profile_id)?
        .rules
        .into_iter()
        .filter(|rule| rule.effective_mode() != AutoAuthMode::Disabled)
        .collect();
    if rules.is_empty() {
        tracing::debug!("AutoAuth: disabled or unconfigured");
        return Ok(None);
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(bridge_error)?;
    listener.set_nonblocking(true).map_err(bridge_error)?;
    let endpoint = listener.local_addr().map_err(bridge_error)?.to_string();
    tracing::debug!(
        rule_count = rules.len(),
        "AutoAuth: configured; bridge available"
    );
    let token = Uuid::new_v4().to_string();
    let worker_token = token.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_shutdown = shutdown.clone();
    let worker = thread::spawn(move || {
        let mut attempted = HashSet::new();
        while !worker_shutdown.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => handle_request(stream, &worker_token, &rules, &mut attempted),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50))
                }
                Err(_) => break,
            }
        }
    });
    Ok(Some(AutoAuthSession {
        endpoint,
        token,
        shutdown,
        worker: Some(worker),
    }))
}

impl AutoAuthSession {
    pub fn configure(&self, command: &mut tokio::process::Command) {
        command
            .env("FLINT_AUTOAUTH_ENDPOINT", &self.endpoint)
            .env("FLINT_AUTOAUTH_TOKEN", &self.token);
    }
}

impl Drop for AutoAuthSession {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn handle_request(
    mut stream: TcpStream,
    token: &str,
    rules: &[StoredRule],
    attempted: &mut HashSet<String>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut line = String::new();
    let parsed = BufReader::new(&stream)
        .take(4096)
        .read_line(&mut line)
        .ok()
        .filter(|count| *count > 0)
        .and_then(|_| serde_json::from_str::<BridgeRequest>(&line).ok());
    let response = parsed
        .map(|request| {
            resolve_request(request, token, rules, attempted, |reference| {
                credential(reference).ok()?.get_password().ok()
            })
        })
        .unwrap_or_else(|| rejected("request_rejected"));
    if let Ok(bytes) = serde_json::to_vec(&response) {
        let _ = stream.write_all(&bytes);
        let _ = stream.write_all(b"\n");
    }
}

fn resolve_request<F>(
    request: BridgeRequest,
    token: &str,
    rules: &[StoredRule],
    attempted: &mut HashSet<String>,
    mut load_credential: F,
) -> BridgeResponse
where
    F: FnMut(&str) -> Option<String>,
{
    if request.token != token {
        tracing::debug!("AutoAuth: authentication failed (invalid bridge token)");
        return rejected("request_rejected");
    }
    let server = normalize_server(&request.server);
    let Some(rule) = rules
        .iter()
        .find(|rule| normalize_server(&rule.server_address) == server)
    else {
        tracing::debug!("AutoAuth: authentication failed (server not configured)");
        return rejected("server_not_configured");
    };
    tracing::debug!("AutoAuth: server matched");
    let Ok(session_id) = Uuid::parse_str(&request.session_id) else {
        tracing::debug!("AutoAuth: authentication failed (invalid session)");
        return rejected("request_rejected");
    };
    if !claim_attempt(attempted, &rule.id, &session_id) {
        tracing::debug!("AutoAuth: authentication failed (replay blocked)");
        return rejected("already_attempted");
    }
    let Some(mut password) = load_credential(&rule.credential_ref) else {
        tracing::debug!("AutoAuth: authentication failed (credential unavailable)");
        return rejected("credential_unavailable");
    };
    tracing::debug!("AutoAuth: credential retrieved");
    let template = match rule.effective_mode() {
        AutoAuthMode::Login => &rule.login_template,
        AutoAuthMode::Register => &rule.registration_template,
        AutoAuthMode::Disabled => {
            password.zeroize();
            return rejected("disabled");
        }
    };
    let command = render_command(template, &password);
    password.zeroize();
    tracing::debug!("AutoAuth: authentication requested");
    BridgeResponse {
        command: Some(command),
        error: None,
    }
}

fn rejected(error: &'static str) -> BridgeResponse {
    BridgeResponse {
        command: None,
        error: Some(error),
    }
}

fn claim_attempt(attempted: &mut HashSet<String>, rule_id: &str, session_id: &Uuid) -> bool {
    attempted.insert(format!("{rule_id}:{session_id}"))
}

fn credential(reference: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, reference).map_err(credential_error)
}

fn credential_error(_error: keyring::Error) -> AppError {
    AppError::new(
        "credential_store_unavailable",
        "The system credential store could not complete the AutoAuth operation.",
    )
}

fn bridge_error(_error: std::io::Error) -> AppError {
    AppError::new(
        "autoauth_bridge_unavailable",
        "Flint could not start its private local AutoAuth bridge.",
    )
}

fn validate_server(value: &str) -> Result<String> {
    let value = normalize_server(value);
    if value.is_empty()
        || value.len() > 255
        || value.contains(char::is_whitespace)
        || value.contains("//")
        || value.contains('/')
        || value.contains('\\')
    {
        return Err(AppError::new(
            "invalid_autoauth_server",
            "Enter a Minecraft server address such as play.example.net or play.example.net:25565.",
        ));
    }
    Ok(value)
}

fn normalize_server(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if let Some(host) = value.strip_suffix(":25565") {
        host.trim_end_matches('.').to_owned()
    } else if value.starts_with('[') {
        value
    } else {
        value.trim_end_matches('.').to_owned()
    }
}

fn validate_template(value: &str, registration: bool) -> Result<()> {
    let required = if registration { 2 } else { 1 };
    if value.len() > 160
        || value.contains(['\r', '\n', '\0'])
        || !value.starts_with('/')
        || value.matches("{password}").count() < required
    {
        return Err(AppError::new(
            "invalid_autoauth_template",
            if registration {
                "Registration commands must start with / and contain {password} twice."
            } else {
                "Login commands must start with / and contain {password}."
            },
        ));
    }
    Ok(())
}

fn render_command(template: &str, password: &str) -> String {
    template
        .trim_start_matches('/')
        .replace("{password}", password)
}

fn load(paths: &AppPaths, profile_id: &str) -> Result<StoredConfig> {
    let path = file(paths, profile_id);
    if !path.is_file() {
        return Ok(StoredConfig {
            protocol_version: PROTOCOL_VERSION,
            rules: Vec::new(),
        });
    }
    let config: StoredConfig = serde_json::from_slice(&fs::read(path)?)?;
    if config.protocol_version != PROTOCOL_VERSION {
        return Err(AppError::new(
            "autoauth_protocol_unsupported",
            "This AutoAuth configuration was created by an unsupported Flint version.",
        ));
    }
    Ok(config)
}

fn write(paths: &AppPaths, profile_id: &str, config: &StoredConfig) -> Result<()> {
    let path = file(paths, profile_id);
    let parent = path.parent().expect("AutoAuth file has a parent");
    fs::create_dir_all(parent)?;
    let temporary = parent.join("autoauth-v1.json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(config)?)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_conservative_and_render_without_a_slash() {
        assert!(validate_template("/login {password}", false).is_ok());
        assert!(validate_template("/register {password} {password}", true).is_ok());
        assert!(validate_template("/register {password}", true).is_err());
        assert!(validate_template("login {password}", false).is_err());
        assert_eq!(
            render_command("/login {password}", "dummy-secret"),
            "login dummy-secret"
        );
    }

    #[test]
    fn server_validation_rejects_urls_and_whitespace() {
        assert_eq!(
            validate_server(" Play.Example.Net:25565 ").unwrap(),
            "play.example.net"
        );
        assert!(validate_server("https://example.net").is_err());
        assert!(validate_server("bad server").is_err());
    }

    #[test]
    fn serialized_rules_never_contain_a_password_field() {
        let config = StoredConfig {
            protocol_version: 1,
            rules: vec![StoredRule {
                id: "rule".into(),
                mode: Some(AutoAuthMode::Login),
                enabled: None,
                server_address: "localhost".into(),
                login_template: "/login {password}".into(),
                registration_template: "/register {password} {password}".into(),
                credential_ref: "opaque-reference".into(),
            }],
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("dummy-secret"));
        assert!(!json.contains("password\""));
        assert!(json.contains("opaque-reference"));
    }

    #[test]
    fn one_command_is_allowed_per_rule() {
        let mut attempts = HashSet::new();
        let first = Uuid::new_v4();
        let reconnect = Uuid::new_v4();
        assert!(claim_attempt(&mut attempts, "rule", &first));
        assert!(!claim_attempt(&mut attempts, "rule", &first));
        assert!(claim_attempt(&mut attempts, "rule", &reconnect));
    }

    #[test]
    fn server_matching_normalizes_case_default_port_and_trailing_dot() {
        assert_eq!(
            normalize_server(" Play.Example.Net:25565 "),
            "play.example.net"
        );
        assert_eq!(normalize_server("play.example.net."), "play.example.net");
    }

    #[test]
    fn legacy_enabled_rules_migrate_to_login_mode() {
        let rule: StoredRule = serde_json::from_str(
            r#"{
            "id":"legacy","enabled":true,"serverAddress":"play.example.net",
            "loginTemplate":"/login {password}",
            "registrationTemplate":"/register {password} {password}",
            "credentialRef":"ref"
        }"#,
        )
        .unwrap();
        assert_eq!(rule.effective_mode(), AutoAuthMode::Login);
    }

    #[test]
    fn bridge_selects_configured_mode_and_blocks_replay() {
        let rules = vec![test_rule(AutoAuthMode::Register)];
        let mut attempts = HashSet::new();
        let session_id = Uuid::new_v4().to_string();
        let response = resolve_request(
            BridgeRequest {
                token: "token".into(),
                server: "PLAY.EXAMPLE.NET:25565".into(),
                session_id: session_id.clone(),
            },
            "token",
            &rules,
            &mut attempts,
            |_| Some("dummy-secret".into()),
        );
        assert_eq!(
            response.command.as_deref(),
            Some("register dummy-secret dummy-secret")
        );
        let replay = resolve_request(
            BridgeRequest {
                token: "token".into(),
                server: "play.example.net".into(),
                session_id,
            },
            "token",
            &rules,
            &mut attempts,
            |_| Some("dummy-secret".into()),
        );
        assert_eq!(replay.error, Some("already_attempted"));
    }

    #[test]
    fn missing_credential_fails_closed_without_a_command() {
        let mut attempts = HashSet::new();
        let response = resolve_request(
            BridgeRequest {
                token: "token".into(),
                server: "play.example.net".into(),
                session_id: Uuid::new_v4().to_string(),
            },
            "token",
            &[test_rule(AutoAuthMode::Login)],
            &mut attempts,
            |_| None,
        );
        assert!(response.command.is_none());
        assert_eq!(response.error, Some("credential_unavailable"));
    }

    fn test_rule(mode: AutoAuthMode) -> StoredRule {
        StoredRule {
            id: "rule".into(),
            mode: Some(mode),
            enabled: None,
            server_address: "play.example.net".into(),
            login_template: "/login {password}".into(),
            registration_template: "/register {password} {password}".into(),
            credential_ref: "ref".into(),
        }
    }

    #[test]
    fn disabled_or_unconfigured_autoauth_starts_no_bridge() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(temp.path());
        paths.ensure().unwrap();
        assert!(start_session(&paths, "unconfigured-profile")
            .unwrap()
            .is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an interactive Windows logon session for Credential Manager"]
    fn windows_credential_manager_round_trip_uses_dummy_secret() {
        let reference = format!("flint-test-{}", Uuid::new_v4());
        let entry = credential(&reference).unwrap();
        entry.set_password("dummy-test-only").unwrap();
        let mut loaded = entry.get_password().unwrap();
        assert_eq!(loaded, "dummy-test-only");
        loaded.zeroize();
        entry.delete_credential().unwrap();
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    }
}
