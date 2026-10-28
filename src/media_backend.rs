//! Shared remdia entry point for playback, metadata and independent subtitle jobs.
use anyhow::{Context, Result};
pub fn engine() -> Result<remdia::Engine> {
    let directory = if let Some(path) = std::env::var_os("REMDIA_PLUGIN_DIR") {
        std::path::PathBuf::from(path)
    } else {
        let executable = std::env::current_exe()?;
        let parent = executable.parent().context("executable directory")?;
        let direct = parent.join("plugins");
        if direct.is_dir() {
            direct
        } else {
            parent.parent().unwrap_or(parent).join("plugins")
        }
    };
    remdia::Engine::discover(&directory, 256 * 1024 * 1024).with_context(|| {
        format!(
            "load remdia component manifests from {}",
            directory.display()
        )
    })
}
