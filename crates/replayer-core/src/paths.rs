//! Application data root shared by caches, downloads and the comic library.
//! Desktop keeps the historical environment-derived location; hosts with an
//! explicit sandbox (Android) install their root once during startup.
use std::{path::PathBuf, sync::OnceLock};

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Install an explicit data root. Only the first call wins; later hosts keep
/// the installed root so storage never silently relocates mid-process.
pub fn set_root(path: PathBuf) {
    let _ = ROOT.set(path);
}

pub fn root() -> PathBuf {
    if let Some(root) = ROOT.get() {
        return root.clone();
    }
    if let Some(path) = std::env::var_os("REPLAYER_SETTINGS_FILE") {
        return PathBuf::from(path).with_extension("");
    }
    if let Some(root) =
        std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
    {
        return PathBuf::from(root).join("replayer");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".".into())
        .join(".config/replayer")
}
