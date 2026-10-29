//! Shared remdia entry point for playback, metadata and independent subtitle jobs.
use anyhow::{Context, Result};
pub fn engine() -> Result<remdia::Engine> {
    let directory = if let Some(path) = std::env::var_os("REMDIA_PLUGIN_DIR") {
        std::path::PathBuf::from(path)
    } else {
        let executable = std::env::current_exe()?;
        let parent = executable.parent().context("executable directory")?;
        let bundle = parent.parent().unwrap_or(parent).join("Frameworks/remdia");
        let direct = parent.join("plugins");
        if cfg!(target_os = "macos")
            && parent.file_name().is_some_and(|n| n == "MacOS")
            && bundle.is_dir()
        {
            bundle
        } else if direct.is_dir() {
            direct
        } else {
            parent.parent().unwrap_or(parent).join("plugins")
        }
    };
    remdia::Engine::bundled_with_plugins(&directory, 256 * 1024 * 1024).with_context(|| {
        format!(
            "register built-in media components and trusted extensions from {}",
            directory.display()
        )
    })
}
