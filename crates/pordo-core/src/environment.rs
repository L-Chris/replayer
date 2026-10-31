//! Pordo's developer overrides, with aliases for scripts written before the rename.
use std::{env, ffi::OsString};

pub fn var_os(name: &str) -> Option<OsString> {
    env::var_os(name).or_else(|| {
        name.strip_prefix("PORDO_")
            .and_then(|suffix| env::var_os(format!("REPLAYER_{suffix}")))
    })
}

pub fn var(name: &str) -> Result<String, env::VarError> {
    match var_os(name) {
        Some(value) => value.into_string().map_err(env::VarError::NotUnicode),
        None => Err(env::VarError::NotPresent),
    }
}
