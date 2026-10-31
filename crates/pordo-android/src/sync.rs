//! App sandbox adapter for the shared desktop WebDAV protocol.
use crate::{comics, failure, guard, json_string, text};
use anyhow::Result;
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::jstring,
};
use pordo_core::{
    jobs,
    sync::{self, Journal, model::Snapshot, portable},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

type Secrets = BTreeMap<String, String>;

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeSyncValidate(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    config: JString<'_>,
) -> jstring {
    let result = guard(|| {
        settings(&serde_json::from_str(&text(&mut env, &config)?)?)?;
        json_string(&mut env, &json!({"valid":true}))
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeSyncProviders(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
) -> jstring {
    let result = guard(|| {
        json_string(
            &mut env,
            &json!(
                sync::Provider::ALL
                    .iter()
                    .map(|p| { json!({"id":p,"label":p.label(),"url":p.url()}) })
                    .collect::<Vec<_>>()
            ),
        )
    });
    failure(&mut env, result)
}

#[derive(Serialize, Deserialize)]
struct Exchange {
    account: String,
    device: String,
    requested_secrets: Secrets,
    report: sync::webdav::Report,
}

fn settings(value: &Value) -> Result<sync::Settings> {
    let mut settings: sync::Settings = serde_json::from_value(value.clone())?;
    settings.password = value["password"].as_str().unwrap_or_default().into();
    settings.passphrase = value["passphrase"].as_str().unwrap_or_default().into();
    settings.validate()?;
    Ok(settings)
}

fn path(account: &str, device: &str) -> Result<PathBuf> {
    sync::journal_file(&pordo_core::paths::root().join("sync"), account, device)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeSyncExchange(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    request: JString<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let request: Value = serde_json::from_str(&text(&mut env, &request)?)?;
        let settings = settings(&request["config"])?;
        if request["probe"].as_bool().unwrap_or(false) {
            jobs::block_on(async {
                sync::bounded(sync::webdav::test(settings), Duration::from_secs(120)).await
            })?;
            return json_string(&mut env, &json!({"tested":true}));
        }
        let mobile: Snapshot = serde_json::from_value(request["snapshot"].clone())?;
        let secrets: Secrets = serde_json::from_value(request["secrets"].clone())?;
        let account = settings.account();
        let device = settings.device_id.clone();
        let path = path(&account, &device)?;
        let mut journal = Journal::load(&path, &device)?;
        {
            let library = comics::locked(comics::library()?)?;
            journal.capture(
                portable::mobile_snapshot(mobile, &journal, &library.entries)?,
                &portable::mobile_defaults(),
            );
            // Persist local operations before touching the network; offline edits survive restart.
            journal.save(&path)?;
        }
        let requested_secrets = secrets.clone();
        let report = jobs::block_on(async {
            sync::bounded(
                sync::webdav::run(settings, journal, secrets),
                Duration::from_secs(120),
            )
            .await
        })?;
        json_string(
            &mut env,
            &serde_json::to_value(Exchange {
                account,
                device,
                requested_secrets,
                report,
            })?,
        )
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeSyncFinish(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    request: JString<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let request: Value = serde_json::from_str(&text(&mut env, &request)?)?;
        let exchange: Exchange = serde_json::from_value(request["exchange"].clone())?;
        let mobile: Snapshot = serde_json::from_value(request["snapshot"].clone())?;
        let secrets: Secrets = serde_json::from_value(request["secrets"].clone())?;
        let path = path(&exchange.account, &exchange.device)?;
        let journal = Journal::load(&path, &exchange.device)?;
        let mut library = comics::locked(comics::library()?)?;
        let applied = portable::finish_mobile(
            journal,
            exchange.report,
            mobile,
            secrets,
            &exchange.requested_secrets,
            &library.entries,
        )?;
        for entry in applied.entries {
            library.save(entry)?;
        }
        let journal = applied.journal;
        journal.save(&path)?;
        json_string(
            &mut env,
            &json!({"snapshot":applied.mobile,"secrets":applied.secrets,"lastSuccess":journal.last_success,
            "pending":journal.digest()!=journal.uploaded}),
        )
    });
    failure(&mut env, result)
}
