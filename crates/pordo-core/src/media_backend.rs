//! Shared remdia entry point for playback, metadata and independent subtitle jobs.
#[cfg(not(target_os = "android"))]
use anyhow::Context;
use anyhow::Result;
pub fn engine() -> Result<remdia::Engine> {
    #[cfg(target_os = "android")]
    {
        // Built-in packs (native, image, android framework bridge, SRC) are
        // statically registered; the embedding library's JNI_OnLoad and
        // Bridge.initialize must have run before opening sessions.
        remdia::Engine::bundled(256 * 1024 * 1024)
    }
    #[cfg(not(target_os = "android"))]
    {
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
}
