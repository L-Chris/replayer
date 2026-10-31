//! Application data root shared by caches, downloads and the comic library.
//! Desktop keeps the historical environment-derived location; hosts with an
//! explicit sandbox (Android) install their root once during startup.
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// New profiles use Pordo; an existing profile keeps its directory and the
/// credential targets derived from that path. Never move live user data.
pub fn application_directory(base: &Path) -> PathBuf {
    let current = base.join("pordo");
    let legacy = base.join("replayer");
    if !current.exists() && legacy.is_dir() {
        legacy
    } else {
        current
    }
}

/// Install an explicit data root. Only the first call wins; later hosts keep
/// the installed root so storage never silently relocates mid-process.
pub fn set_root(path: PathBuf) {
    let _ = ROOT.set(path);
}

pub fn root() -> PathBuf {
    if let Some(root) = ROOT.get() {
        return root.clone();
    }
    if let Some(path) = crate::environment::var_os("PORDO_SETTINGS_FILE") {
        return PathBuf::from(path).with_extension("");
    }
    if let Some(root) =
        std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
    {
        return application_directory(&PathBuf::from(root));
    }
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".".into())
        .join(".config");
    application_directory(&base)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renamed_app_keeps_existing_profiles_without_moving_them() {
        let base = std::env::temp_dir().join(format!("pordo-paths-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        assert_eq!(application_directory(&base), base.join("pordo"));
        let legacy = base.join("replayer");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("settings.json"), b"existing user data").unwrap();
        assert_eq!(application_directory(&base), legacy);
        assert_eq!(
            std::fs::read(legacy.join("settings.json")).unwrap(),
            b"existing user data"
        );
        std::fs::create_dir(base.join("pordo")).unwrap();
        assert_eq!(application_directory(&base), base.join("pordo"));
        std::fs::remove_dir_all(base).unwrap();
    }
}
