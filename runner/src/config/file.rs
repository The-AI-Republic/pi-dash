use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::schema::{Config, Credentials};
use crate::util::paths::Paths;

pub fn load_config(paths: &Paths) -> Result<Config> {
    let path = paths.config_path();
    let text = fs::read_to_string(&path).with_context(|| format!("reading config at {path:?}"))?;
    let cfg: Config = toml::from_str(&text).with_context(|| format!("parsing {path:?}"))?;
    Ok(cfg)
}

pub fn load_config_opt(paths: &Paths) -> Result<Option<Config>> {
    match load_config(paths) {
        Ok(c) => Ok(Some(c)),
        Err(e) => {
            if matches!(
                e.downcast_ref::<std::io::Error>().map(|e| e.kind()),
                Some(std::io::ErrorKind::NotFound)
            ) {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

pub fn load_credentials(paths: &Paths) -> Result<Credentials> {
    let path = paths.credentials_path();
    let text =
        fs::read_to_string(&path).with_context(|| format!("reading credentials at {path:?}"))?;
    let creds: Credentials = toml::from_str(&text).with_context(|| format!("parsing {path:?}"))?;
    Ok(creds)
}

pub fn load_all(paths: &Paths) -> Result<(Config, Credentials)> {
    Ok((load_config(paths)?, load_credentials(paths)?))
}

pub fn write_config(paths: &Paths, config: &Config) -> Result<()> {
    paths.ensure()?;
    let s = toml::to_string_pretty(config)?;
    write_private(&paths.config_path(), s.as_bytes())
}

pub fn write_credentials(paths: &Paths, creds: &Credentials) -> Result<()> {
    paths.ensure()?;
    let s = toml::to_string_pretty(creds)?;
    write_private(&paths.credentials_path(), s.as_bytes())
}

pub fn remove_all(paths: &Paths) -> Result<()> {
    for p in [paths.config_path(), paths.credentials_path()] {
        if p.exists() {
            fs::remove_file(&p).with_context(|| format!("removing {p:?}"))?;
        }
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {parent:?}"))?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        use std::io::Write;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    // `rename` preserves the destination's mode on some filesystems; enforce 0600 again.
    let mut perm = fs::metadata(path)?.permissions();
    use std::os::unix::fs::PermissionsExt;
    perm.set_mode(0o600);
    fs::set_permissions(path, perm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{Credentials, RunnerSection, WorkspaceSection};
    use tempfile::tempdir;

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
        }
    }

    #[test]
    fn writes_and_reads_config_with_0600() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let cfg = Config {
            version: 1,
            runner: RunnerSection {
                name: "t".into(),
                cloud_url: "https://x".into(),
            },
            workspace: WorkspaceSection {
                working_dir: tmp.path().join("wd"),
            },
            codex: Default::default(),
            approval_policy: Default::default(),
            logging: Default::default(),
        };
        write_config(&paths, &cfg).unwrap();
        let loaded = load_config(&paths).unwrap();
        assert_eq!(loaded.runner.name, "t");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(paths.config_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn writes_and_reads_credentials() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let creds = Credentials {
            runner_id: uuid::Uuid::new_v4(),
            runner_secret: "s".into(),
            issued_at: chrono::Utc::now(),
        };
        write_credentials(&paths, &creds).unwrap();
        let loaded = load_credentials(&paths).unwrap();
        assert_eq!(loaded.runner_secret, "s");
    }
}
