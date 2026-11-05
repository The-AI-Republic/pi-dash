//! Install a CLI release's engine version directory.
//!
//! Every CLI release archive (`pidash-<target>.tar.xz`, `.zip` on Windows)
//! carries `engine/<version>/pidash-agent-engine[.exe]` with Codex's
//! `LICENSE` and `NOTICE`. The cargo-dist 0.22 shell and PowerShell
//! installers only copy the package's executables into the install directory
//! and drop everything else in the archive, so the engine directory is placed
//! by `pidash` itself:
//!
//! - `pidash engine install` — run by the `install.sh` / `install.ps1`
//!   wrappers right after the cargo-dist installer, and by hand after a bare
//!   `pidash-installer.sh` / `.ps1` install;
//! - `pidash update` — for the *new* release, before the cargo-dist installer
//!   replaces `pidash`, so the new binary finds its engine on first start.
//!
//! The MSI installs the engine directory itself (`runner/wix/main.wxs`).
//!
//! The archive is fetched from the same GitHub release as `pidash` and
//! checked against the release's `<archive>.sha256`, the same check the
//! cargo-dist installers make. Only the archive's `engine/` tree is used.
//! A version directory that already holds an engine is never touched, so a
//! daemon running it is never given a swapped binary; a new version is moved
//! into place with a single rename from a staging directory inside the engine
//! root.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

/// Base of the release download URLs: `<base>/pidash-v<version>/<asset>`.
pub const DEFAULT_RELEASE_DOWNLOAD_URL: &str =
    concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/download");

/// Overrides [`DEFAULT_RELEASE_DOWNLOAD_URL`] (a mirror, or a local server in
/// tests).
pub const RELEASE_DOWNLOAD_URL_ENV: &str = "PIDASH_RELEASE_DOWNLOAD_URL";

/// Target triple this binary was built for (`build.rs`).
pub const BUILD_TARGET: &str = env!("PIDASH_BUILD_TARGET");

/// cargo-dist's archive name for a target.
pub fn archive_name(target: &str) -> String {
    if target.contains("windows") {
        format!("pidash-{target}.zip")
    } else {
        format!("pidash-{target}.tar.xz")
    }
}

/// URL of a CLI release's archive for a target.
pub fn release_archive_url(base: &str, cli_version: &str, target: &str) -> String {
    format!(
        "{}/pidash-v{}/{}",
        base.trim_end_matches('/'),
        cli_version.trim_start_matches('v'),
        archive_name(target)
    )
}

/// The engine root for the running executable: where a fresh install goes.
pub fn install_root_for_current_exe() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating the pidash executable")?;
    // Follow a symlinked `pidash` to the install it belongs to.
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let dir = exe
        .parent()
        .context("pidash executable has no parent directory")?;
    Ok(super::locate::install_root(dir, cfg!(windows)))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct InstallOutcome {
    /// Versions moved into place by this call.
    pub installed: Vec<String>,
    /// Versions that were already installed and left untouched.
    pub already_present: Vec<String>,
}

/// Download the release archive of `cli_version` for this build's target and
/// install its engine version directories under `root`.
pub async fn install_release(cli_version: &str, root: &Path) -> Result<InstallOutcome> {
    let base = std::env::var(RELEASE_DOWNLOAD_URL_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_RELEASE_DOWNLOAD_URL.to_string());
    install_release_from(&base, cli_version, BUILD_TARGET, root, cfg!(windows)).await
}

async fn install_release_from(
    base: &str,
    cli_version: &str,
    target: &str,
    root: &Path,
    windows: bool,
) -> Result<InstallOutcome> {
    let url = release_archive_url(base, cli_version, target);

    std::fs::create_dir_all(root)
        .with_context(|| format!("creating engine directory {}", root.display()))?;
    let staging = Staging::create(root)?;

    let archive = staging.path().join(archive_name(target));
    download_verified(&url, &archive).await?;

    let extracted = staging.path().join("x");
    std::fs::create_dir_all(&extracted)?;
    extract(&archive, &extracted).await?;

    install_from_extracted(&extracted, root, windows)
        .with_context(|| format!("installing the engine from {url}"))
}

/// Move every `engine/<version>/` found in an extracted release archive into
/// `root/<version>/`. `extracted` may hold `engine/` directly (the Windows
/// zip) or inside one top-level directory (the `pidash-<target>/` of the
/// tarballs). `extracted` must be on the same filesystem as `root`.
pub fn install_from_extracted(
    extracted: &Path,
    root: &Path,
    windows: bool,
) -> Result<InstallOutcome> {
    let engine_dir = find_engine_dir(extracted)?;
    let binary_name = super::binary_name(windows);
    let mut outcome = InstallOutcome::default();

    let mut versions: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(&engine_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if super::parse_version(&name).is_none() {
            continue;
        }
        let binary = entry.path().join(binary_name);
        let is_file = std::fs::symlink_metadata(&binary)
            .map(|m| m.file_type().is_file())
            .unwrap_or(false);
        if !is_file {
            bail!(
                "release archive's engine/{name}/ has no regular {binary_name}; refusing to install it"
            );
        }
        versions.push((name, entry.path()));
    }
    if versions.is_empty() {
        bail!("release archive's engine/ directory holds no engine version");
    }
    versions.sort();

    std::fs::create_dir_all(root)?;
    for (name, src) in versions {
        let dest = root.join(&name);
        if dest.join(binary_name).is_file() {
            outcome.already_present.push(name);
            continue;
        }
        if dest.exists() {
            // A version directory without its binary is a leftover from an
            // interrupted manual copy; nothing can be running from it.
            std::fs::remove_dir_all(&dest)
                .with_context(|| format!("removing incomplete {}", dest.display()))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                src.join(binary_name),
                std::fs::Permissions::from_mode(0o755),
            )?;
        }
        std::fs::rename(&src, &dest)
            .with_context(|| format!("moving engine {name} into {}", dest.display()))?;
        outcome.installed.push(name);
    }
    Ok(outcome)
}

fn find_engine_dir(extracted: &Path) -> Result<PathBuf> {
    let direct = extracted.join("engine");
    if direct.is_dir() {
        return Ok(direct);
    }
    let mut found = Vec::new();
    for entry in std::fs::read_dir(extracted)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let candidate = entry.path().join("engine");
            if candidate.is_dir() {
                found.push(candidate);
            }
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => bail!(
            "this release archive carries no engine/ directory (releases before the engine shipped with the CLI do not)"
        ),
        _ => bail!("release archive holds more than one engine/ directory"),
    }
}

/// A `.staging-<uuid>` directory inside the engine root, removed on drop.
/// Inside the root so the final move is a same-filesystem rename; the
/// leading dot and non-version name keep the locator and cleanup off it.
struct Staging(PathBuf);

impl Staging {
    fn create(root: &Path) -> Result<Self> {
        let path = root.join(format!(".staging-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path)
            .with_context(|| format!("creating staging directory {}", path.display()))?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("pidash/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        // The archive is ~95 MB; bound a stalled transfer, not a slow one.
        .read_timeout(Duration::from_secs(120))
        .build()
        .context("building HTTP client")
}

/// Expected digest from a cargo-dist `.sha256` file (`<hex> *<name>`).
pub fn parse_sha256_file(body: &str) -> Result<String> {
    let digest = body.split_whitespace().next().unwrap_or_default();
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("malformed checksum file: {body:?}");
    }
    Ok(digest.to_ascii_lowercase())
}

async fn download_verified(url: &str, dest: &Path) -> Result<()> {
    let client = http_client()?;
    let sha_url = format!("{url}.sha256");
    let expected = client
        .get(&sha_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("downloading {sha_url}"))?
        .text()
        .await
        .with_context(|| format!("reading {sha_url}"))?;
    let expected = parse_sha256_file(&expected)?;

    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("downloading {url}"))?;
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("creating {}", dest.display()))?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("downloading {url}"))?
    {
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);

    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        bail!("checksum mismatch for {url}: expected {expected}, got {actual}");
    }
    Ok(())
}

/// `tar` reads both the `.tar.xz` archives and the Windows `.zip` (bsdtar).
/// The cargo-dist installers depend on the same tool. On Windows, prefer the
/// system bsdtar: a Git-for-Windows GNU tar earlier on PATH cannot read zips.
fn tar_program() -> PathBuf {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let system_tar = PathBuf::from(root).join("System32").join("tar.exe");
        if system_tar.is_file() {
            return system_tar;
        }
    }
    PathBuf::from("tar")
}

async fn extract(archive: &Path, into: &Path) -> Result<()> {
    let output = tokio::process::Command::new(tar_program())
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .output()
        .await
        .context("running tar to unpack the release archive")?;
    if !output.status.success() {
        bail!(
            "tar could not unpack {}: {}",
            archive.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn stage_archive_tree(top: &Path, version: &str, windows: bool) -> PathBuf {
        let dir = top.join("engine").join(version);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(crate::engine::binary_name(windows)), b"new engine").unwrap();
        fs::write(dir.join("LICENSE"), b"Apache-2.0").unwrap();
        fs::write(dir.join("NOTICE"), b"notice").unwrap();
        dir
    }

    #[test]
    fn archive_names_follow_cargo_dist() {
        assert_eq!(
            archive_name("x86_64-unknown-linux-musl"),
            "pidash-x86_64-unknown-linux-musl.tar.xz"
        );
        assert_eq!(
            archive_name("x86_64-pc-windows-msvc"),
            "pidash-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            release_archive_url(
                "https://github.com/The-AI-Republic/pi-dash/releases/download/",
                "0.1.30",
                "aarch64-apple-darwin"
            ),
            "https://github.com/The-AI-Republic/pi-dash/releases/download/pidash-v0.1.30/pidash-aarch64-apple-darwin.tar.xz"
        );
        assert!(DEFAULT_RELEASE_DOWNLOAD_URL.ends_with("/pi-dash/releases/download"));
    }

    #[test]
    fn sha256_file_parsing() {
        let hex = "ab".repeat(32);
        assert_eq!(
            parse_sha256_file(&format!("{hex} *pidash-x.tar.xz\n")).unwrap(),
            hex
        );
        assert_eq!(parse_sha256_file(&hex.to_uppercase()).unwrap(), hex);
        assert!(parse_sha256_file("nope").is_err());
        assert!(parse_sha256_file("").is_err());
    }

    #[test]
    fn installs_from_a_tarball_layout_beside_the_old_version() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lib/pidash/engine");
        let old = root.join("0.150.0");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("pidash-agent-engine"), b"old engine").unwrap();

        let extracted = root.join(".staging-test/x");
        stage_archive_tree(
            &extracted.join("pidash-x86_64-unknown-linux-gnu"),
            "0.153.4",
            false,
        );

        let outcome = install_from_extracted(&extracted, &root, false).unwrap();
        assert_eq!(outcome.installed, vec!["0.153.4".to_string()]);
        let new = root.join("0.153.4");
        assert_eq!(
            fs::read(new.join("pidash-agent-engine")).unwrap(),
            b"new engine"
        );
        assert!(new.join("LICENSE").is_file() && new.join("NOTICE").is_file());
        // The old version is left for the daemon's cleanup.
        assert_eq!(
            fs::read(old.join("pidash-agent-engine")).unwrap(),
            b"old engine"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(new.join("pidash-agent-engine"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn installs_from_a_zip_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("bin/engine");
        let extracted = root.join(".staging-test/x");
        stage_archive_tree(&extracted, "0.153.4", true);

        let outcome = install_from_extracted(&extracted, &root, true).unwrap();
        assert_eq!(outcome.installed, vec!["0.153.4".to_string()]);
        assert!(root.join("0.153.4/pidash-agent-engine.exe").is_file());
    }

    #[test]
    fn an_installed_version_is_never_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("engine");
        let current = root.join("0.153.4");
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("pidash-agent-engine"), b"running engine").unwrap();
        let extracted = tmp.path().join("x");
        stage_archive_tree(&extracted, "0.153.4", false);

        let outcome = install_from_extracted(&extracted, &root, false).unwrap();
        assert_eq!(outcome.already_present, vec!["0.153.4".to_string()]);
        assert!(outcome.installed.is_empty());
        assert_eq!(
            fs::read(current.join("pidash-agent-engine")).unwrap(),
            b"running engine"
        );
    }

    #[test]
    fn an_incomplete_version_dir_is_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("engine");
        fs::create_dir_all(root.join("0.153.4")).unwrap();
        fs::write(root.join("0.153.4/LICENSE"), b"partial").unwrap();
        let extracted = root.join(".staging-test/x");
        stage_archive_tree(&extracted, "0.153.4", false);

        let outcome = install_from_extracted(&extracted, &root, false).unwrap();
        assert_eq!(outcome.installed, vec!["0.153.4".to_string()]);
        assert!(root.join("0.153.4/pidash-agent-engine").is_file());
    }

    #[test]
    fn an_archive_without_an_engine_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let extracted = tmp.path().join("x/pidash-x86_64-unknown-linux-gnu");
        fs::create_dir_all(&extracted).unwrap();
        fs::write(extracted.join("pidash"), b"cli").unwrap();
        let err = install_from_extracted(&tmp.path().join("x"), &tmp.path().join("engine"), false)
            .unwrap_err();
        assert!(err.to_string().contains("no engine/"), "{err}");
    }

    #[test]
    fn a_version_dir_without_the_binary_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let extracted = tmp.path().join("x");
        fs::create_dir_all(extracted.join("engine/0.153.4")).unwrap();
        fs::write(extracted.join("engine/0.153.4/LICENSE"), b"x").unwrap();
        let root = tmp.path().join("engine-root");
        assert!(install_from_extracted(&extracted, &root, false).is_err());
        assert!(!root.join("0.153.4").exists());
    }

    #[test]
    fn non_version_entries_in_the_archive_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let extracted = tmp.path().join("x");
        stage_archive_tree(&extracted, "0.153.4", false);
        fs::create_dir_all(extracted.join("engine/not-a-version")).unwrap();
        let root = tmp.path().join("engine-root");
        let outcome = install_from_extracted(&extracted, &root, false).unwrap();
        assert_eq!(outcome.installed, vec!["0.153.4".to_string()]);
        assert!(!root.join("not-a-version").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn extract_unpacks_a_tarball_with_the_system_tar() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        stage_archive_tree(
            &src.join("pidash-x86_64-unknown-linux-gnu"),
            "0.153.4",
            false,
        );
        let archive = tmp.path().join("a.tar.gz");
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .arg("pidash-x86_64-unknown-linux-gnu")
            .status()
            .unwrap();
        assert!(status.success());

        let into = tmp.path().join("x");
        fs::create_dir_all(&into).unwrap();
        extract(&archive, &into).await.unwrap();
        let outcome = install_from_extracted(&into, &tmp.path().join("engine"), false).unwrap();
        assert_eq!(outcome.installed, vec!["0.153.4".to_string()]);
    }

    /// Serve `files` (path -> body) over plain HTTP/1.1 on localhost; returns
    /// the base URL. Enough of a server for reqwest's GETs.
    #[cfg(unix)]
    async fn serve(files: Vec<(String, Vec<u8>)>) -> String {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let files = files.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                    let (status, body) = match files.iter().find(|(p, _)| *p == path) {
                        Some((_, b)) => ("200 OK", b.clone()),
                        None => ("404 Not Found", Vec::new()),
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    #[cfg(unix)]
    fn tarball(target: &str, version: &str) -> Vec<u8> {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        stage_archive_tree(&src.join(format!("pidash-{target}")), version, false);
        let archive = tmp.path().join("a.tar.gz");
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .arg(format!("pidash-{target}"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::read(archive).unwrap()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_release_downloads_verifies_and_installs() {
        let target = "x86_64-unknown-linux-gnu";
        let body = tarball(target, "0.153.4");
        let digest = format!("{:x}", Sha256::digest(&body));
        let asset = format!("/pidash-v0.1.30/{}", archive_name(target));
        let base = serve(vec![
            (asset.clone(), body),
            (
                format!("{asset}.sha256"),
                format!("{digest} *x\n").into_bytes(),
            ),
        ])
        .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lib/pidash/engine");
        let outcome = install_release_from(&base, "0.1.30", target, &root, false)
            .await
            .unwrap();
        assert_eq!(outcome.installed, vec!["0.153.4".to_string()]);
        assert!(root.join("0.153.4/pidash-agent-engine").is_file());
        // Staging is cleaned up; only the version directory remains.
        let names: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["0.153.4".to_string()]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_release_rejects_a_checksum_mismatch() {
        let target = "x86_64-unknown-linux-gnu";
        let body = tarball(target, "0.153.4");
        let asset = format!("/pidash-v0.1.30/{}", archive_name(target));
        let base = serve(vec![
            (asset.clone(), body),
            (format!("{asset}.sha256"), "0".repeat(64).into_bytes()),
        ])
        .await;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("engine");
        let err = install_release_from(&base, "0.1.30", target, &root, false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"), "{err:#}");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }
}
