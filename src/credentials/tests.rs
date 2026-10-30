use super::*;
use anyhow::bail;
use std::{cell::RefCell, collections::HashMap};

#[derive(Default)]
pub(crate) struct MemoryVault {
    pub values: RefCell<HashMap<String, String>>,
    pub fail: RefCell<Option<String>>,
}
impl Vault for MemoryVault {
    fn get(&self, target: &str) -> Result<Option<String>> {
        Ok(self.values.borrow().get(target).cloned())
    }
    fn set(&self, target: &str, value: Option<&str>) -> Result<()> {
        if self.fail.borrow().as_deref() == Some(target) {
            bail!("Simulated vault failure");
        }
        match value {
            Some(value) => {
                self.values.borrow_mut().insert(target.into(), value.into());
            }
            None => {
                self.values.borrow_mut().remove(target);
            }
        }
        Ok(())
    }
}

#[test]
fn saving_failure_rolls_back_key_changes_and_deletions() {
    let vault = MemoryVault::default();
    vault.set("a", Some("old-key")).unwrap();
    vault.set("b", Some("other-key")).unwrap();
    let before = vault.values.borrow().clone();
    let changes = vec![
        ("a".into(), "new-key".into()),
        ("b".into(), String::new()),
        ("c".into(), "third-key".into()),
    ];
    assert!(transaction(&vault, &changes, || bail!("Disk full")).is_err());
    assert!(before == *vault.values.borrow());
    *vault.fail.borrow_mut() = Some("b".into());
    assert!(transaction(&vault, &changes, || Ok(())).is_err());
    assert!(before == *vault.values.borrow());
}

#[test]
#[ignore = "Writes and removes a temporary test-only OS credential"]
#[cfg(any(windows, target_os = "macos"))]
fn system_vault_roundtrip() {
    let target = format!("replayer/test/{}", uuid::Uuid::new_v4());
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = SystemVault.set(&self.0, None);
        }
    }
    let _cleanup = Cleanup(target.clone());
    assert!(SystemVault.get(&target).unwrap().is_none());
    SystemVault.set(&target, Some("test-only-key")).unwrap();
    assert!(SystemVault.get(&target).unwrap().as_deref() == Some("test-only-key"));
    SystemVault
        .set(&target, Some("replacement-test-key"))
        .unwrap();
    assert!(SystemVault.get(&target).unwrap().as_deref() == Some("replacement-test-key"));
    SystemVault.set(&target, None).unwrap();
    assert!(SystemVault.get(&target).unwrap().is_none());
}
