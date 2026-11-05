//! Where the daemon and engine credentials are kept (design §4.3,
//! `[auth] credentials_store`).
//!
//! The secrets are the `[cli]` tokens: the default workspace's `mt_` token,
//! every `[[cli.workspaces]]` token and the engine credential (`ec_`).
//! Per-runner refresh tokens stay in their own 0600 files.
//!
//! | `credentials_store` | Where the secrets go                                                         |
//! | ------------------- | ---------------------------------------------------------------------------- |
//! | `auto` (default)    | The OS keychain once a sign-in moved them there and while it answers, else `config.toml` (0600) |
//! | `keyring`           | The OS keychain only; a write fails when it does not answer                   |
//! | `file`              | `config.toml` (0600), as before                                              |
//!
//! The rest of the runner never sees the difference: [`hydrate`] fills the
//! secrets into the in-memory [`Config`] when `config.toml` is loaded, and
//! [`plan_write`] takes them out again when it is written. While they are in
//! the keychain the file records that (`[auth] secrets_in = "keyring"`) and
//! which items it holds (`keyring_items`), and keeps every non-secret field
//! (workspace slugs, the engine credential's expiry).
//!
//! **Migration.** Under `auto`, plaintext tokens move into the keychain at
//! the next sign-in ([`super::file::migrate_credentials_store`], called by
//! `pidash auth login`), never as a side effect of another command.
//!
//! **Fallback.** A keychain that does not answer (a headless Linux server
//! with no Secret Service, a locked keyring under a systemd user service) is
//! detected by a probe with a timeout; `auto` then keeps the file, and
//! `pidash doctor` says so. Secrets already in a keychain that stopped
//! answering are never overwritten with blanks: the write fails instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use super::schema::{
    AuthSection, Config, CredentialsStoreMode, EngineCredential, SECRETS_IN_KEYRING,
    WorkspaceCredential,
};
use crate::util::paths::Paths;

/// Keychain service name of every item.
pub const KEYRING_SERVICE: &str = "pidash";
/// Item of the default workspace's daemon credential.
pub const ITEM_DEFAULT_MACHINE_TOKEN: &str = "machine-token";
/// Prefix of the items of the other workspaces' daemon credentials.
pub const ITEM_WORKSPACE_PREFIX: &str = "machine-token:";
/// Item of the engine credential.
pub const ITEM_ENGINE_CREDENTIAL: &str = "engine-credential";
/// Item read to find out whether the keychain answers.
const PROBE_ITEM: &str = "probe";
/// `PIDASH_KEYRING=off` makes the keychain report itself unavailable (CI,
/// containers, a user who wants the file without editing the config).
pub const KEYRING_ENV: &str = "PIDASH_KEYRING";
/// How long one keychain call may take before it counts as not answering.
const KEYRING_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("the OS keychain is not available: {0}")]
pub struct SecretError(pub String);

/// One OS credential store (or a stand-in for tests).
pub trait SecretBackend: Send + Sync {
    /// Human name, for `pidash doctor`.
    fn name(&self) -> &'static str;
    /// `Ok(None)` when the store answers but holds no such item.
    fn get(&self, item: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, item: &str, value: &str) -> Result<(), SecretError>;
    /// Deleting a missing item is not an error.
    fn delete(&self, item: &str) -> Result<(), SecretError>;
    /// Whether the store answers at all.
    fn probe(&self) -> Result<(), SecretError> {
        self.get(PROBE_ITEM).map(|_| ())
    }
}

/// The real OS keychain through the `keyring` crate. Items are scoped by the
/// config directory, so two isolated installs never share a credential.
struct OsKeyring {
    scope: String,
}

impl OsKeyring {
    fn user(&self, item: &str) -> String {
        format!("{item}@{}", self.scope)
    }
}

/// Run one keychain call on its own thread with a timeout: a Secret Service
/// that is present but stuck must not hang a CLI command or the daemon.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn bounded<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, SecretError> + Send + 'static,
) -> Result<T, SecretError> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(KEYRING_TIMEOUT)
        .unwrap_or_else(|_| Err(SecretError("the keychain did not answer in time".into())))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl SecretBackend for OsKeyring {
    fn name(&self) -> &'static str {
        if cfg!(target_os = "macos") {
            "macOS Keychain"
        } else if cfg!(target_os = "windows") {
            "Windows Credential Manager"
        } else {
            "Secret Service"
        }
    }

    fn get(&self, item: &str) -> Result<Option<String>, SecretError> {
        let user = self.user(item);
        bounded(move || {
            let entry = keyring::Entry::new(KEYRING_SERVICE, &user)
                .map_err(|e| SecretError(e.to_string()))?;
            match entry.get_password() {
                Ok(v) => Ok(Some(v)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(SecretError(e.to_string())),
            }
        })
    }

    fn set(&self, item: &str, value: &str) -> Result<(), SecretError> {
        let user = self.user(item);
        let value = value.to_string();
        bounded(move || {
            keyring::Entry::new(KEYRING_SERVICE, &user)
                .and_then(|e| e.set_password(&value))
                .map_err(|e| SecretError(e.to_string()))
        })
    }

    fn delete(&self, item: &str) -> Result<(), SecretError> {
        let user = self.user(item);
        bounded(move || {
            let entry = keyring::Entry::new(KEYRING_SERVICE, &user)
                .map_err(|e| SecretError(e.to_string()))?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(SecretError(e.to_string())),
            }
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
impl SecretBackend for OsKeyring {
    fn name(&self) -> &'static str {
        "OS keychain"
    }
    fn get(&self, _item: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError("no keychain support on this platform".into()))
    }
    fn set(&self, _item: &str, _value: &str) -> Result<(), SecretError> {
        Err(SecretError("no keychain support on this platform".into()))
    }
    fn delete(&self, _item: &str) -> Result<(), SecretError> {
        Err(SecretError("no keychain support on this platform".into()))
    }
}

/// A keychain that never answers (`PIDASH_KEYRING=off`).
struct Disabled;

impl SecretBackend for Disabled {
    fn name(&self) -> &'static str {
        "disabled"
    }
    fn get(&self, _item: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError(format!("{KEYRING_ENV}=off")))
    }
    fn set(&self, _item: &str, _value: &str) -> Result<(), SecretError> {
        Err(SecretError(format!("{KEYRING_ENV}=off")))
    }
    fn delete(&self, _item: &str) -> Result<(), SecretError> {
        Err(SecretError(format!("{KEYRING_ENV}=off")))
    }
}

/// In-memory keychain for tests. `set_available(false)` makes it behave
/// like a headless Linux box with no Secret Service.
#[derive(Default)]
pub struct MemoryKeyring {
    items: Mutex<HashMap<String, String>>,
    unavailable: std::sync::atomic::AtomicBool,
}

impl MemoryKeyring {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn unavailable() -> Arc<Self> {
        let k = Self::default();
        k.set_available(false);
        Arc::new(k)
    }

    pub fn set_available(&self, available: bool) {
        self.unavailable
            .store(!available, std::sync::atomic::Ordering::SeqCst);
    }

    /// Snapshot of the stored items.
    pub fn items(&self) -> HashMap<String, String> {
        self.items.lock().expect("memory keyring poisoned").clone()
    }

    fn check(&self) -> Result<(), SecretError> {
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
            Err(SecretError("no Secret Service on this test host".into()))
        } else {
            Ok(())
        }
    }
}

impl SecretBackend for MemoryKeyring {
    fn name(&self) -> &'static str {
        "in-memory test keyring"
    }
    fn get(&self, item: &str) -> Result<Option<String>, SecretError> {
        self.check()?;
        Ok(self
            .items
            .lock()
            .expect("memory keyring poisoned")
            .get(item)
            .cloned())
    }
    fn set(&self, item: &str, value: &str) -> Result<(), SecretError> {
        self.check()?;
        self.items
            .lock()
            .expect("memory keyring poisoned")
            .insert(item.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, item: &str) -> Result<(), SecretError> {
        self.check()?;
        self.items
            .lock()
            .expect("memory keyring poisoned")
            .remove(item);
        Ok(())
    }
}

type Overrides = Mutex<HashMap<PathBuf, Arc<dyn SecretBackend>>>;

fn overrides() -> &'static Overrides {
    static O: OnceLock<Overrides> = OnceLock::new();
    O.get_or_init(Default::default)
}

/// Use `backend` for the install whose config directory is `config_dir`
/// (tests: each test's temp dir gets its own keychain, and the real one is
/// never touched).
pub fn use_backend_for(config_dir: &Path, backend: Arc<dyn SecretBackend>) {
    overrides()
        .lock()
        .expect("keyring overrides poisoned")
        .insert(config_dir.to_path_buf(), backend);
}

/// The keychain for this install.
pub fn backend_for(paths: &Paths) -> Arc<dyn SecretBackend> {
    if let Some(b) = overrides()
        .lock()
        .expect("keyring overrides poisoned")
        .get(&paths.config_dir)
    {
        return b.clone();
    }
    // Unit tests never reach the developer's real keychain.
    if cfg!(test) || std::env::var(KEYRING_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        return Arc::new(Disabled);
    }
    Arc::new(OsKeyring {
        scope: paths.config_dir.display().to_string(),
    })
}

/// Every secret in `cfg`, as `(item, value)`.
fn collect(cfg: &Config) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(cli) = cfg.cli.as_ref() else {
        return out;
    };
    if let Some(t) = cli.token.as_deref().filter(|t| !t.is_empty()) {
        out.push((ITEM_DEFAULT_MACHINE_TOKEN.to_string(), t.to_string()));
    }
    for w in &cli.workspaces {
        if !w.token.is_empty() && !w.slug.is_empty() {
            out.push((
                format!("{ITEM_WORKSPACE_PREFIX}{}", w.slug),
                w.token.clone(),
            ));
        }
    }
    if let Some(ec) = cli
        .engine_credential
        .as_ref()
        .filter(|c| !c.token.is_empty())
    {
        out.push((ITEM_ENGINE_CREDENTIAL.to_string(), ec.token.clone()));
    }
    out
}

/// Take every secret out of `cfg` (the copy that is written to the file).
fn blank(cfg: &mut Config) {
    if let Some(cli) = cfg.cli.as_mut() {
        if cli.token.as_deref().is_some_and(|t| !t.is_empty()) {
            cli.token = None;
        }
        for w in &mut cli.workspaces {
            w.token.clear();
        }
        if let Some(ec) = cli.engine_credential.as_mut() {
            ec.token.clear();
        }
    }
}

/// Put `value` back where `item` belongs in `cfg`.
fn assign(cfg: &mut Config, item: &str, value: String) {
    let cli = cfg.cli.get_or_insert_with(Default::default);
    if item == ITEM_DEFAULT_MACHINE_TOKEN {
        cli.token = Some(value);
    } else if item == ITEM_ENGINE_CREDENTIAL {
        cli.engine_credential
            .get_or_insert_with(EngineCredential::default)
            .token = value;
    } else if let Some(slug) = item.strip_prefix(ITEM_WORKSPACE_PREFIX) {
        match cli.workspaces.iter_mut().find(|w| w.slug == slug) {
            Some(w) => w.token = value,
            None => cli.workspaces.push(WorkspaceCredential {
                slug: slug.to_string(),
                token: value,
            }),
        }
    }
}

/// Fill the secrets of a freshly parsed `cfg` in from the keychain, when the
/// file says they are there. A keychain that does not answer leaves them
/// empty (the daemon then runs signed out) and is reported, not fatal.
pub fn hydrate(paths: &Paths, cfg: &mut Config) -> Result<(), SecretError> {
    if !cfg.auth.secrets_in_keyring() {
        return Ok(());
    }
    let backend = backend_for(paths);
    for item in cfg.auth.keyring_items.clone() {
        if let Some(value) = backend.get(&item)? {
            assign(cfg, &item, value);
        }
    }
    Ok(())
}

/// What [`super::file::write_config`] writes, and what to clean up after.
pub struct WritePlan {
    /// The config to serialise: secrets blanked when they are in the
    /// keychain, and `[auth]` describing where they are.
    pub config: Config,
    /// Keychain items to delete once the file is written.
    pub delete_after: Vec<String>,
}

/// Decide where `cfg`'s secrets go and store them there.
///
/// `on_disk` is the `[auth]` table of the file being replaced (the truth
/// about where the secrets are now). `migrate` is set by a sign-in: under
/// `auto` it moves plaintext tokens into a keychain that answers.
pub fn plan_write(
    paths: &Paths,
    cfg: &Config,
    on_disk: &AuthSection,
    migrate: bool,
) -> anyhow::Result<WritePlan> {
    let mode = cfg.auth.credentials_store;
    let now_in_keyring = on_disk.secrets_in_keyring();
    let secrets = collect(cfg);
    let backend = backend_for(paths);
    let needs_keyring = now_in_keyring
        || mode == CredentialsStoreMode::Keyring
        || (mode == CredentialsStoreMode::Auto && migrate && !secrets.is_empty());
    let keyring_ok = if needs_keyring {
        backend.probe()
    } else {
        Err(SecretError("not probed".into()))
    };
    if now_in_keyring && let Err(e) = &keyring_ok {
        // The secrets are in a keychain that no longer answers: they were
        // not loaded, so writing the file now would replace them with
        // nothing.
        anyhow::bail!(
            "the credentials are stored in the {} and it is not answering ({e}); \
             unlock it (or sign in to the desktop session) and try again",
            backend.name()
        );
    }
    let to_keyring = match mode {
        CredentialsStoreMode::File => false,
        CredentialsStoreMode::Keyring => {
            if let Err(e) = &keyring_ok {
                anyhow::bail!(
                    "[auth] credentials_store = \"keyring\" but the {} is not available ({e}); \
                     set credentials_store = \"auto\" or \"file\" to use config.toml",
                    backend.name()
                );
            }
            true
        }
        CredentialsStoreMode::Auto => (now_in_keyring || migrate) && keyring_ok.is_ok(),
    };

    let mut out = cfg.clone();
    if to_keyring {
        let mut items = Vec::with_capacity(secrets.len());
        for (item, value) in &secrets {
            backend
                .set(item, value)
                .map_err(|e| anyhow::anyhow!("storing {item} in the {}: {e}", backend.name()))?;
            items.push(item.clone());
        }
        items.sort();
        let delete_after = on_disk
            .keyring_items
            .iter()
            .filter(|i| !items.contains(i))
            .cloned()
            .collect();
        blank(&mut out);
        out.auth.secrets_in = (!items.is_empty()).then(|| SECRETS_IN_KEYRING.to_string());
        out.auth.keyring_items = items;
        Ok(WritePlan {
            config: out,
            delete_after,
        })
    } else {
        out.auth.secrets_in = None;
        out.auth.keyring_items.clear();
        let delete_after = if now_in_keyring {
            on_disk.keyring_items.clone()
        } else {
            Vec::new()
        };
        Ok(WritePlan {
            config: out,
            delete_after,
        })
    }
}

/// Delete the keychain items a write left behind. Best-effort.
pub fn finish_write(paths: &Paths, plan: &WritePlan) {
    if plan.delete_after.is_empty() {
        return;
    }
    let backend = backend_for(paths);
    for item in &plan.delete_after {
        if let Err(e) = backend.delete(item) {
            tracing::warn!(item, "deleting a stale keychain item failed: {e}");
        }
    }
}

/// What `pidash doctor` reports about the credential store.
pub struct StoreReport {
    pub ok: bool,
    pub detail: String,
}

/// Which store holds the credentials of `cfg`, and whether that matches
/// `[auth] credentials_store`.
pub fn describe(paths: &Paths, cfg: &Config) -> StoreReport {
    let mode = cfg.auth.credentials_store;
    let backend = backend_for(paths);
    if cfg.auth.secrets_in_keyring() {
        return match backend.probe() {
            Ok(()) => StoreReport {
                ok: true,
                detail: format!(
                    "credentials_store = {}: in the {} ({} item(s))",
                    mode.as_str(),
                    backend.name(),
                    cfg.auth.keyring_items.len()
                ),
            },
            Err(e) => StoreReport {
                ok: false,
                detail: format!(
                    "credentials_store = {}: in the {}, which is not answering ({e})",
                    mode.as_str(),
                    backend.name()
                ),
            },
        };
    }
    let file = paths.config_path().display().to_string();
    match mode {
        CredentialsStoreMode::File => StoreReport {
            ok: true,
            detail: format!("credentials_store = file: in {file} (0600)"),
        },
        CredentialsStoreMode::Keyring => StoreReport {
            ok: collect(cfg).is_empty(),
            detail: match backend.probe() {
                Ok(()) => format!(
                    "credentials_store = keyring: the {} answers; credentials move there at the next `pidash auth login`",
                    backend.name()
                ),
                Err(e) => format!(
                    "credentials_store = keyring but the {} is not available ({e})",
                    backend.name()
                ),
            },
        },
        CredentialsStoreMode::Auto => match backend.probe() {
            Ok(()) => StoreReport {
                ok: true,
                detail: format!(
                    "credentials_store = auto: in {file} (0600); the {} answers, so the next \
                     `pidash auth login` moves them there",
                    backend.name()
                ),
            },
            Err(e) => StoreReport {
                ok: true,
                detail: format!(
                    "credentials_store = auto: in {file} (0600) — fallback, no keychain answered ({e})"
                ),
            },
        },
    }
}

/// Short name of where the secrets of `cfg` are: `"keychain"` or `"file"`.
pub fn location(cfg: &Config) -> &'static str {
    if cfg.auth.secrets_in_keyring() {
        "keychain"
    } else {
        "file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::file::{
        load_config, migrate_credentials_store, mutate_config, write_config,
    };
    use crate::config::schema::{CliSection, DaemonConfig};

    fn paths(root: &Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    fn signed_in(mode: CredentialsStoreMode) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://cloud.example".into(),
                ..DaemonConfig::default()
            },
            runners: vec![],
            cli: Some(CliSection {
                token: Some("mt_acme".into()),
                workspace_slug: Some("acme".into()),
                workspaces: vec![WorkspaceCredential {
                    slug: "beta".into(),
                    token: "mt_beta".into(),
                }],
                engine_credential: Some(EngineCredential {
                    token: "ec_secret".into(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::days(14)),
                }),
                ..Default::default()
            }),
            engine: Default::default(),
            auth: AuthSection {
                credentials_store: mode,
                ..Default::default()
            },
        }
    }

    fn file_text(p: &Paths) -> String {
        std::fs::read_to_string(p.config_path()).unwrap()
    }

    fn setup(
        mode: CredentialsStoreMode,
        keyring: Arc<MemoryKeyring>,
    ) -> (tempfile::TempDir, Paths) {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        use_backend_for(&p.config_dir, keyring);
        write_config(&p, &signed_in(mode)).unwrap();
        (tmp, p)
    }

    type Tokens = (Option<String>, Option<String>, Option<String>);

    fn tokens(cfg: &Config) -> Tokens {
        let cli = cfg.cli.clone().unwrap();
        (
            cli.token,
            cli.workspaces.first().map(|w| w.token.clone()),
            cli.engine_credential.map(|c| c.token),
        )
    }

    #[test]
    fn file_mode_keeps_the_tokens_in_the_0600_file() {
        let keyring = MemoryKeyring::new();
        let (_tmp, p) = setup(CredentialsStoreMode::File, keyring.clone());
        migrate_credentials_store(&p).unwrap();
        assert!(file_text(&p).contains("ec_secret"));
        assert!(
            keyring.items().is_empty(),
            "file mode never uses the keychain"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(p.config_path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn auto_moves_plaintext_into_the_keychain_at_the_next_sign_in_only() {
        let keyring = MemoryKeyring::new();
        let (_tmp, p) = setup(CredentialsStoreMode::Auto, keyring.clone());
        // An ordinary write (e.g. `runner add`) does not migrate.
        mutate_config(&p, |_| Ok(())).unwrap();
        assert!(file_text(&p).contains("mt_acme"));
        assert!(keyring.items().is_empty());

        // The next sign-in does.
        let (_, location) = migrate_credentials_store(&p).unwrap();
        assert_eq!(location, "keychain");
        let text = file_text(&p);
        for secret in ["mt_acme", "mt_beta", "ec_secret"] {
            assert!(!text.contains(secret), "{secret} left in the file:\n{text}");
        }
        assert!(text.contains("expires_at"), "non-secret fields stay");
        assert!(text.contains("slug = \"beta\""));
        let items = keyring.items();
        assert_eq!(
            items.get(ITEM_DEFAULT_MACHINE_TOKEN).map(String::as_str),
            Some("mt_acme")
        );
        assert_eq!(
            items.get("machine-token:beta").map(String::as_str),
            Some("mt_beta")
        );
        assert_eq!(
            items.get(ITEM_ENGINE_CREDENTIAL).map(String::as_str),
            Some("ec_secret")
        );

        // Every reader sees the tokens as before.
        let cfg = load_config(&p).unwrap();
        assert_eq!(
            tokens(&cfg),
            (
                Some("mt_acme".into()),
                Some("mt_beta".into()),
                Some("ec_secret".into())
            )
        );
        assert_eq!(
            cfg.cli.as_ref().unwrap().token_for_workspace(Some("beta")),
            Some("mt_beta")
        );

        // Later ordinary writes keep them in the keychain, and a signed-out
        // workspace's item is deleted.
        mutate_config(&p, |cfg| {
            cfg.cli.as_mut().unwrap().remove_workspace_token("beta");
            Ok(())
        })
        .unwrap();
        assert!(!file_text(&p).contains("mt_acme"));
        assert!(!keyring.items().contains_key("machine-token:beta"));
        assert!(keyring.items().contains_key(ITEM_DEFAULT_MACHINE_TOKEN));
    }

    #[test]
    fn auto_falls_back_cleanly_to_the_file_without_a_keychain() {
        // A headless Linux server: no Secret Service.
        let keyring = MemoryKeyring::unavailable();
        let (_tmp, p) = setup(CredentialsStoreMode::Auto, keyring);
        let (_, location) = migrate_credentials_store(&p).unwrap();
        assert_eq!(location, "file");
        assert!(file_text(&p).contains("ec_secret"));
        let cfg = load_config(&p).unwrap();
        assert_eq!(tokens(&cfg).0.as_deref(), Some("mt_acme"));
        let report = describe(&p, &cfg);
        assert!(report.ok);
        assert!(report.detail.contains("fallback"), "{}", report.detail);
    }

    #[test]
    fn keyring_mode_refuses_to_fall_back() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        use_backend_for(&p.config_dir, MemoryKeyring::unavailable());
        let err = write_config(&p, &signed_in(CredentialsStoreMode::Keyring)).unwrap_err();
        assert!(
            err.to_string().contains("credentials_store = \"keyring\""),
            "{err}"
        );
        assert!(!p.config_path().exists(), "nothing written in plaintext");

        // With a keychain, `keyring` stores there on any write.
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let keyring = MemoryKeyring::new();
        use_backend_for(&p.config_dir, keyring.clone());
        write_config(&p, &signed_in(CredentialsStoreMode::Keyring)).unwrap();
        assert!(!file_text(&p).contains("ec_secret"));
        assert_eq!(keyring.items().len(), 3);
        assert!(
            describe(&p, &load_config(&p).unwrap())
                .detail
                .contains("in the in-memory test keyring")
        );
    }

    #[test]
    fn a_keychain_that_stops_answering_is_never_overwritten_with_blanks() {
        let keyring = MemoryKeyring::new();
        let (_tmp, p) = setup(CredentialsStoreMode::Auto, keyring.clone());
        migrate_credentials_store(&p).unwrap();
        keyring.set_available(false);
        // Loading continues, signed out.
        let cfg = load_config(&p).unwrap();
        assert_eq!(tokens(&cfg).0, None);
        // Writing refuses rather than dropping the secrets.
        let err = mutate_config(&p, |_| Ok(())).unwrap_err();
        assert!(format!("{err:#}").contains("not answering"), "{err:#}");
        keyring.set_available(true);
        assert_eq!(
            tokens(&load_config(&p).unwrap()).2.as_deref(),
            Some("ec_secret")
        );
    }

    #[test]
    fn switching_to_file_moves_the_secrets_back_and_clears_the_keychain() {
        let keyring = MemoryKeyring::new();
        let (_tmp, p) = setup(CredentialsStoreMode::Auto, keyring.clone());
        migrate_credentials_store(&p).unwrap();
        mutate_config(&p, |cfg| {
            cfg.auth.credentials_store = CredentialsStoreMode::File;
            Ok(())
        })
        .unwrap();
        assert!(file_text(&p).contains("ec_secret"));
        assert!(keyring.items().is_empty());
        assert_eq!(location(&load_config(&p).unwrap()), "file");
    }

    #[test]
    fn keychain_off_reports_unavailable() {
        assert!(Disabled.probe().is_err());
    }

    /// Round trip through the real OS keychain. Ignored by default (CI has
    /// none, and tests must not write to a developer's keychain); run with
    /// `cargo test os_keychain_round_trip -- --ignored` on a desktop
    /// session. Cleans up after itself.
    #[test]
    #[ignore]
    fn os_keychain_round_trip() {
        let backend = OsKeyring {
            scope: format!("pidash-test-{}", uuid::Uuid::new_v4()),
        };
        backend
            .probe()
            .expect("no OS keychain answers on this host");
        assert_eq!(backend.get("item").unwrap(), None);
        backend.set("item", "secret-value").unwrap();
        assert_eq!(
            backend.get("item").unwrap().as_deref(),
            Some("secret-value")
        );
        backend.delete("item").unwrap();
        assert_eq!(backend.get("item").unwrap(), None);
        backend.delete("item").unwrap();
    }
}
