//! Read configuration without mutating the process environment or logging secrets.
use anyhow::{Result, anyhow};
use std::path::PathBuf;

pub fn get(name: &str) -> Result<Option<String>> {
    if let Ok(value) = std::env::var(name)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }
    let file = if let Some(path) = std::env::var_os("REPLAYER_ENV_FILE") {
        Some(PathBuf::from(path))
    } else {
        let cwd = std::env::current_dir().ok();
        let exe = std::env::current_exe().ok();
        cwd.iter()
            .flat_map(|p| p.ancestors())
            .chain(
                exe.iter()
                    .filter_map(|p| p.parent())
                    .flat_map(|p| p.ancestors()),
            )
            .map(|p| p.join(".env"))
            .find(|p| p.is_file())
    };
    let mut value = None;
    if let Some(path) = file {
        for entry in dotenvy::from_path_iter(path).map_err(|_| anyhow!("Cannot read .env"))? {
            let (key, item) = entry.map_err(|_| anyhow!("Cannot parse .env"))?;
            if key == name && !item.trim().is_empty() {
                value = Some(item);
            }
        }
    }
    Ok(value)
}
