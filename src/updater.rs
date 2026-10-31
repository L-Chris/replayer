use anyhow::{Context, Result, ensure};
use aws_lc_rs::digest::{Context as DigestContext, SHA256};
use crossbeam_channel::{Receiver, Sender, unbounded};
use futures_util::StreamExt;
use semver::Version;
use serde::Deserialize;
use std::{io::Write, path::PathBuf, time::Duration};

pub const REPOSITORY: &str = "https://github.com/PordoTech/pordo";
const API: &str = "https://api.github.com/repos/PordoTech/pordo/releases/latest";
const MAX_SIZE: u64 = 512 * 1024 * 1024;

#[derive(Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
    size: u64,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Clone)]
pub struct Update {
    pub version: String,
    pub notes: String,
    asset: Asset,
}
enum Event {
    Checked(Result<Option<Update>, String>),
    Progress(u64, u64),
    Downloaded(Result<PathBuf, String>),
}
#[derive(Default)]
pub enum Status {
    #[default]
    Idle,
    Checking,
    Current,
    Available,
    Downloading {
        received: u64,
        total: u64,
    },
    Ready,
    Failed(String),
}
pub struct Updater {
    pub status: Status,
    pub update: Option<Update>,
    installer: Option<PathBuf>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    pub dialog_visible: bool,
    prompted_version: Option<String>,
    manual_check: bool,
    repaint: eframe::egui::Context,
}
impl Updater {
    pub fn new(automatic: bool, repaint: eframe::egui::Context) -> Self {
        let (tx, rx) = unbounded();
        let mut value = Self {
            status: Status::Idle,
            update: None,
            installer: None,
            tx,
            rx,
            dialog_visible: false,
            prompted_version: None,
            manual_check: false,
            repaint,
        };
        if automatic && cfg!(target_os = "windows") && !cfg!(debug_assertions) {
            value.start_check(false);
        }
        value
    }
    pub fn busy(&self) -> bool {
        matches!(self.status, Status::Checking | Status::Downloading { .. })
    }
    pub fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Checked(Ok(update)) => {
                    if let Some(update) = &update {
                        self.dialog_visible = self.manual_check
                            || self.prompted_version.as_deref() != Some(&update.version);
                        self.prompted_version = Some(update.version.clone());
                    } else {
                        self.dialog_visible = false;
                    }
                    self.status = if update.is_some() {
                        Status::Available
                    } else {
                        Status::Current
                    };
                    self.update = update;
                }
                Event::Checked(Err(error)) | Event::Downloaded(Err(error)) => {
                    self.status = Status::Failed(error)
                }
                Event::Progress(received, total) => {
                    self.status = Status::Downloading { received, total }
                }
                Event::Downloaded(Ok(path)) => {
                    self.installer = Some(path);
                    self.status = Status::Ready;
                }
            }
        }
    }
    pub fn check(&mut self) {
        self.start_check(true);
    }
    fn start_check(&mut self, manual: bool) {
        if self.busy() {
            return;
        }
        self.update = None;
        self.installer = None;
        self.status = Status::Checking;
        self.manual_check = manual;
        self.dialog_visible = false;
        let tx = self.tx.clone();
        let repaint = self.repaint.clone();
        spawn(move || {
            let result = runtime().and_then(|rt| rt.block_on(check()));
            let _ = tx.send(Event::Checked(result.map_err(|e| format!("{e:#}"))));
            repaint.request_repaint();
        });
    }
    pub fn download(&mut self) {
        if self.busy() {
            return;
        }
        let Some(update) = self.update.clone() else {
            return;
        };
        self.status = Status::Downloading {
            received: 0,
            total: update.asset.size,
        };
        self.dialog_visible = true;
        let tx = self.tx.clone();
        let repaint = self.repaint.clone();
        spawn(move || {
            let result = runtime().and_then(|rt| rt.block_on(download(&update, &tx)));
            let _ = tx.send(Event::Downloaded(result.map_err(|e| format!("{e:#}"))));
            repaint.request_repaint();
        });
    }
    pub fn install(&mut self) -> Result<()> {
        let update = self.update.as_ref().context("No update selected")?;
        let installer = self
            .installer
            .as_ref()
            .context("Download the update first")?;
        let result = launch_installer(update, installer);
        if let Err(error) = &result {
            self.status = Status::Failed(format!("{error:#}"));
        }
        result
    }
    pub fn can_install(&self) -> bool {
        self.installer.is_some()
    }
    #[cfg(test)]
    pub(crate) fn offer_fixture(&mut self) {
        self.tx.send(Event::Checked(Ok(Some(Update {
            version: "9.0.0".into(),
            notes: "## Fix\n\n- Keep controls visible.\n\n<details>\n<summary>中文更新说明</summary>\n\n## Fix\n\n- 修复按钮遮挡。\n\n</details>".into(),
            asset: Asset { name: String::new(), browser_download_url: String::new(), digest: None, size: 0 },
        })))).unwrap();
        self.poll();
    }
}
fn spawn(work: impl FnOnce() + Send + 'static) {
    std::thread::spawn(work);
}
fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}
fn client(timeout: Duration) -> Result<reqwest::Client> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    Ok(reqwest::Client::builder()
        .user_agent(concat!("pordo/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() >= 5 {
                return attempt.error("Too many update redirects");
            }
            if url.scheme() == "https"
                && url.host_str().is_some_and(|host| {
                    host == "github.com" || host.ends_with(".githubusercontent.com")
                })
            {
                attempt.follow()
            } else {
                attempt.error("Untrusted update redirect")
            }
        }))
        .build()?)
}
async fn check() -> Result<Option<Update>> {
    let response = client(Duration::from_secs(20))?
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let mut stream = response.error_for_status()?.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        ensure!(
            bytes.len() + chunk.len() <= 2 * 1024 * 1024,
            "Release metadata too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    select_release(serde_json::from_slice(&bytes)?, env!("CARGO_PKG_VERSION"))
}
fn select_release(release: Release, current: &str) -> Result<Option<Update>> {
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let tag = release
        .tag_name
        .strip_prefix('v')
        .context("Release tag must start with v")?;
    let version = Version::parse(tag)?;
    if !version.pre.is_empty() || !version.build.is_empty() || version <= Version::parse(current)? {
        return Ok(None);
    }
    let expected = format!("pordo-{version}-windows-x86_64-setup.exe");
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == expected)
        .context("Release is missing the Windows x64 installer")?;
    let update = Update {
        version: version.to_string(),
        notes: release.body.unwrap_or_default(),
        asset,
    };
    validate(&update)?;
    Ok(Some(update))
}
fn validate(update: &Update) -> Result<()> {
    let asset = &update.asset;
    ensure!(
        asset.name == format!("pordo-{}-windows-x86_64-setup.exe", update.version),
        "Unexpected installer name"
    );
    ensure!(
        asset.size > 0 && asset.size <= MAX_SIZE,
        "Invalid installer size"
    );
    let digest = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .context("Release has no SHA-256 digest")?;
    ensure!(
        digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid SHA-256 digest"
    );
    let url = reqwest::Url::parse(&asset.browser_download_url)?;
    let expected = format!(
        "/PordoTech/pordo/releases/download/v{}/{}",
        update.version, asset.name
    );
    ensure!(
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == expected,
        "Untrusted installer URL"
    );
    Ok(())
}
fn verify(update: &Update, size: u64, hash: &str) -> Result<()> {
    validate(update)?;
    ensure!(size == update.asset.size, "Installer size mismatch");
    ensure!(
        update.asset.digest.as_deref().unwrap()[7..].eq_ignore_ascii_case(hash),
        "Installer SHA-256 mismatch"
    );
    Ok(())
}
async fn download(update: &Update, tx: &Sender<Event>) -> Result<PathBuf> {
    validate(update)?;
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .context("LOCALAPPDATA is unavailable")?;
    let cache = pordo_core::paths::application_directory(&base)
        .join("updates")
        .join(&update.version);
    std::fs::create_dir_all(&cache)?;
    let path = cache.join(&update.asset.name);
    let partial = path.with_extension(format!("{}.part", std::process::id()));
    let outcome = async {
        let response = client(Duration::from_secs(600))?
            .get(&update.asset.browser_download_url)
            .send()
            .await?
            .error_for_status()?;
        if let Some(size) = response.content_length() {
            ensure!(size == update.asset.size, "Unexpected download length");
        }
        let mut file = std::fs::File::create(&partial)?;
        let mut hash = DigestContext::new(&SHA256);
        let mut size = 0u64;
        let mut last = std::time::Instant::now();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            size += chunk.len() as u64;
            ensure!(
                size <= update.asset.size && size <= MAX_SIZE,
                "Update download exceeds size limit"
            );
            hash.update(&chunk);
            file.write_all(&chunk)?;
            if last.elapsed() >= Duration::from_millis(150) {
                let _ = tx.send(Event::Progress(size, update.asset.size));
                last = std::time::Instant::now();
            }
        }
        verify(update, size, &crate::crypto::digest_hex(hash.finish()))?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&partial, &path)?;
        Ok(path)
    }
    .await;
    if outcome.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    outcome
}
#[cfg(windows)]
fn launch_installer(update: &Update, path: &std::path::Path) -> Result<()> {
    use std::{io::Read, os::windows::process::CommandExt};
    let mut file = std::fs::File::open(path)?;
    let mut hash = DigestContext::new(&SHA256);
    let mut size = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        ensure!(size <= MAX_SIZE, "Installer too large");
        hash.update(&buffer[..n]);
    }
    verify(update, size, &crate::crypto::digest_hex(hash.finish()))?;
    let executable = std::env::current_exe()?;
    let install_dir = executable
        .parent()
        .context("Missing application directory")?;
    let helper = path.with_extension(format!("{}.ps1", std::process::id()));
    std::fs::write(&helper, include_str!("../scripts/install-update.ps1"))?;
    let powershell =
        PathBuf::from(std::env::var_os("SystemRoot").context("SystemRoot unavailable")?)
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    std::process::Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&helper)
        .env("PORDO_UPDATE_PID", std::process::id().to_string())
        .env("PORDO_UPDATE_INSTALLER", path)
        .env("PORDO_UPDATE_DIR", install_dir)
        .env(
            "PORDO_UPDATE_SHA256",
            &update.asset.digest.as_deref().unwrap()[7..],
        )
        .env("PORDO_UPDATE_RELAUNCH", install_dir.join("pordo.exe"))
        .creation_flags(0x08000000)
        .spawn()
        .context("Unable to start update installer")?;
    Ok(())
}
#[cfg(not(windows))]
fn launch_installer(_: &Update, _: &std::path::Path) -> Result<()> {
    anyhow::bail!("Automatic installation currently supports Windows only")
}

pub fn localized_notes(notes: &str, chinese: bool) -> &str {
    if let Some((english, rest)) = notes.split_once("<details>")
        && let Some(rest) = rest
            .trim_start()
            .strip_prefix("<summary>中文更新说明</summary>")
        && let Some((translation, _)) = rest.split_once("</details>")
    {
        return if chinese {
            translation.trim()
        } else {
            english.trim()
        };
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_prompts_once_per_version_manual_checks_reopen_and_failures_keep_retry_state() {
        let mut updater = Updater::new(false, Default::default());
        updater.offer_fixture();
        assert!(updater.dialog_visible);
        updater.dialog_visible = false;
        updater.offer_fixture();
        assert!(!updater.dialog_visible);
        updater.manual_check = true;
        updater.offer_fixture();
        assert!(updater.dialog_visible);
        updater
            .tx
            .send(Event::Downloaded(Err("network unavailable".into())))
            .unwrap();
        updater.poll();
        assert!(updater.update.is_some());
        assert!(updater.dialog_visible);
        assert!(!updater.can_install());
        updater
            .tx
            .send(Event::Downloaded(Ok(PathBuf::from("fixture-installer"))))
            .unwrap();
        updater.poll();
        assert!(updater.can_install());
        updater.status = Status::Failed("installer failed".into());
        assert!(updater.can_install());
        updater.dialog_visible = false;
        updater
            .tx
            .send(Event::Checked(Err("offline".into())))
            .unwrap();
        updater.poll();
        assert!(!updater.dialog_visible);
    }
    fn release(version: &str) -> Release {
        let data = b"installer";
        let name = format!("pordo-{version}-windows-x86_64-setup.exe");
        Release {
            tag_name: format!("v{version}"),
            body: None,
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: name.clone(),
                browser_download_url: format!("{REPOSITORY}/releases/download/v{version}/{name}"),
                digest: Some(format!("sha256:{}", crate::crypto::sha256_hex(data))),
                size: data.len() as u64,
            }],
        }
    }
    #[test]
    fn releases_require_new_stable_version_and_trusted_installer() {
        assert!(
            select_release(release("0.10.0"), "0.9.0")
                .unwrap()
                .is_some()
        );
        assert!(select_release(release("0.1.0"), "0.1.0").unwrap().is_none());
        assert!(select_release(release("0.1.0"), "0.2.0").unwrap().is_none());
        let mut draft = release("0.2.0");
        draft.draft = true;
        assert!(select_release(draft, "0.1.0").unwrap().is_none());
        let mut bad = release("0.2.0");
        bad.assets[0].digest = None;
        assert!(select_release(bad, "0.1.0").is_err());
        for repository in [
            "AnotherOwner/pordo",
            "PordoTech/other",
            "PordoTech/pordo-fork",
            "L-Chris/replayer",
        ] {
            let mut bad = release("0.2.0");
            bad.assets[0].browser_download_url = format!(
                "https://github.com/{repository}/releases/download/v0.2.0/{}",
                bad.assets[0].name,
            );
            assert!(select_release(bad, "0.1.0").is_err());
        }
        let mut bad = release("0.2.0");
        bad.assets[0].browser_download_url = bad.assets[0]
            .browser_download_url
            .replace("github.com", "evil.example");
        assert!(select_release(bad, "0.1.0").is_err());
        let mut bad = release("0.2.0");
        bad.assets[0].size = MAX_SIZE + 1;
        assert!(select_release(bad, "0.1.0").is_err());
    }
    #[test]
    fn installer_integrity_and_localized_notes() {
        let update = select_release(release("0.2.0"), "0.1.0").unwrap().unwrap();
        assert!(verify(&update, 9, &crate::crypto::sha256_hex(b"installer")).is_ok());
        assert!(verify(&update, 9, &crate::crypto::sha256_hex(b"tampered!")).is_err());
        assert!(verify(&update, 8, &crate::crypto::sha256_hex(b"installer")).is_err());
        let notes = "English\n<details>\n<summary>中文更新说明</summary>\n中文\n</details>";
        assert_eq!(localized_notes(notes, true), "中文");
        assert_eq!(localized_notes(notes, false), "English");
    }
}
