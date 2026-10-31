//! Secrets live in the OS vault, never in settings JSON or diagnostics.
use anyhow::{Context, Result};

pub trait Vault {
    fn get(&self, target: &str) -> Result<Option<String>>;
    fn set(&self, target: &str, value: Option<&str>) -> Result<()>;
}

pub struct SystemVault;

/// Roll back key changes if saving settings fails, so the applied config stays usable.
pub fn transaction(
    vault: &impl Vault,
    changes: &[(String, String)],
    commit: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let previous: Vec<_> = changes
        .iter()
        .map(|(target, _)| vault.get(target))
        .collect::<Result<_>>()?;
    for (index, (target, value)) in changes.iter().enumerate() {
        let next = (!value.is_empty()).then_some(value.as_str());
        if previous[index].as_deref() != next
            && let Err(error) = vault.set(target, next)
        {
            rollback(vault, changes, &previous, index)?;
            return Err(error);
        }
    }
    if let Err(error) = commit() {
        rollback(vault, changes, &previous, changes.len())?;
        return Err(error);
    }
    Ok(())
}

fn rollback(
    vault: &impl Vault,
    changes: &[(String, String)],
    previous: &[Option<String>],
    count: usize,
) -> Result<()> {
    for index in (0..count).rev() {
        vault
            .set(&changes[index].0, previous[index].as_deref())
            .context("密钥保存失败，无法恢复原配置 / Cannot restore previous credentials")?;
    }
    Ok(())
}

#[cfg(windows)]
impl Vault for SystemVault {
    fn get(&self, target: &str) -> Result<Option<String>> {
        use windows_sys::Win32::{
            Foundation::{ERROR_NOT_FOUND, GetLastError},
            Security::Credentials::*,
        };
        let target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        let mut credential = std::ptr::null_mut();
        // Pointers refer to live UTF-16 input and an output owned by CredFree.
        unsafe {
            if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
                let error = GetLastError();
                if error == ERROR_NOT_FOUND {
                    return Ok(None);
                }
                anyhow::bail!("无法读取已保存的密钥 / Cannot read saved API key: {error}");
            }
            let blob = &*credential;
            let result = if blob.CredentialBlobSize == 0 {
                Ok(String::new())
            } else {
                String::from_utf8(
                    std::slice::from_raw_parts(
                        blob.CredentialBlob,
                        blob.CredentialBlobSize as usize,
                    )
                    .to_vec(),
                )
                .context("已保存的密钥格式无效 / Invalid saved API key")
            };
            CredFree(credential.cast());
            result.map(Some)
        }
    }
    fn set(&self, target: &str, value: Option<&str>) -> Result<()> {
        use windows_sys::Win32::{
            Foundation::{ERROR_NOT_FOUND, GetLastError},
            Security::Credentials::*,
        };
        let mut target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        unsafe {
            if let Some(value) = value {
                anyhow::ensure!(
                    value.len() <= CRED_MAX_CREDENTIAL_BLOB_SIZE as usize,
                    "密钥过长 / API key too long"
                );
                let credential = CREDENTIALW {
                    Type: CRED_TYPE_GENERIC,
                    TargetName: target.as_mut_ptr(),
                    CredentialBlobSize: value.len() as u32,
                    CredentialBlob: value.as_ptr().cast_mut(),
                    Persist: CRED_PERSIST_LOCAL_MACHINE,
                    ..std::mem::zeroed()
                };
                if CredWriteW(&credential, 0) == 0 {
                    anyhow::bail!("无法保存密钥 / Cannot save API key: {}", GetLastError());
                }
            } else if CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) == 0 {
                let error = GetLastError();
                if error != ERROR_NOT_FOUND {
                    anyhow::bail!("无法清除密钥 / Cannot clear API key: {error}");
                }
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use security_framework::passwords::{self, PasswordOptions};
    // Security.framework's errSecItemNotFound.
    const ITEM_NOT_FOUND: i32 = -25300;

    fn options(target: &str) -> PasswordOptions {
        // Published Keychain service: changing this would hide existing credentials.
        let mut options = PasswordOptions::new_generic_password("dev.replayer.credentials", target);
        options.set_access_synchronized(Some(false));
        options
    }
    impl Vault for SystemVault {
        fn get(&self, target: &str) -> Result<Option<String>> {
            match passwords::generic_password(options(target)) {
                Ok(bytes) => String::from_utf8(bytes)
                    .map(Some)
                    .context("已保存的凭据格式无效 / Invalid saved credential"),
                Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
                Err(error) => Err(error).context("无法读取钥匙串 / Cannot read Keychain"),
            }
        }
        fn set(&self, target: &str, value: Option<&str>) -> Result<()> {
            let result = match value {
                Some(value) => {
                    passwords::set_generic_password_options(value.as_bytes(), options(target))
                }
                None => passwords::delete_generic_password_options(options(target)),
            };
            match result {
                Ok(()) => Ok(()),
                Err(error) if value.is_none() && error.code() == ITEM_NOT_FOUND => Ok(()),
                Err(error) => Err(error).context("无法更新钥匙串 / Cannot update Keychain"),
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
impl Vault for SystemVault {
    fn get(&self, _target: &str) -> Result<Option<String>> {
        Ok(None)
    }
    fn set(&self, _target: &str, value: Option<&str>) -> Result<()> {
        if value.is_some() {
            anyhow::bail!(
                "此平台暂不支持保存密钥 / Credential storage is not supported on this platform"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;
