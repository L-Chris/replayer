//! Android JNI bridge: exposes pordo-core (comic/music sources, bookshelf,
//! caches, downloads, lyrics) and remedia playback sessions to the Kotlin app
//! as `libpordo_android.so`.
//!
//! Conventions shared with the Kotlin side:
//! - Text parameters and results are JSON strings; binary results are
//!   containers: 4-byte big-endian JSON length, JSON metadata, raw payload.
//! - Every call is synchronous. Async core work runs on the shared worker
//!   runtime via `jobs::block_on`, so JNI callers must be ordinary threads.
//! - Failures throw `IllegalStateException` with the same bilingual messages
//!   the desktop app displays.
//! - `dev.remdia.Bridge.initialize(context)` must have run (see the app's
//!   `dev.remdia` bootstrap) before opening media or comic image sessions.
#[cfg(target_os = "android")]
mod comics;
#[cfg(target_os = "android")]
mod media;
#[cfg(target_os = "android")]
mod music;
#[cfg(target_os = "android")]
mod sync;

#[cfg(target_os = "android")]
remdia_android_platform::jni_on_load!();

#[cfg(target_os = "android")]
use anyhow::{Context, Result};
#[cfg(target_os = "android")]
use jni::{
    JNIEnv,
    objects::{JClass, JObject, JString},
    sys::{jbyteArray, jstring},
};

/// Report errors as Java exceptions; the return value is the type's default.
#[cfg(target_os = "android")]
pub(crate) fn failure<T: Default>(env: &mut JNIEnv<'_>, result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            let _ = env.throw_new("java/lang/IllegalStateException", format!("{error:#}"));
            T::default()
        }
    }
}

/// Minimal logcat writer so native panics and diagnostics survive in logs.
#[cfg(target_os = "android")]
mod android_log {
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: std::os::raw::c_int, tag: *const u8, text: *const u8) -> i32;
    }
    pub fn write(prio: i32, tag: &str, text: &str) {
        let tag = std::ffi::CString::new(tag).unwrap_or_default();
        let text = std::ffi::CString::new(text).unwrap_or_default();
        unsafe { __android_log_write(prio, tag.as_ptr().cast(), text.as_ptr().cast()) };
    }
    pub fn error(tag: &str, text: &str) {
        write(6, tag, text);
    }

    /// Route the `log` ecosystem (notably rustls-platform-verifier, whose
    /// Android side logs the platform's own message for each failed
    /// verification) into logcat instead of dropping it.
    pub struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            let prio = match record.level() {
                log::Level::Error => 6,
                log::Level::Warn => 5,
                log::Level::Info => 4,
                log::Level::Debug | log::Level::Trace => 3,
            };
            write(
                prio,
                "pordo-rust",
                &format!("[{}] {}", record.target(), record.args()),
            );
        }
        fn flush(&self) {}
    }
    pub static LOGGER: Logger = Logger;
}

/// Run a JNI body without letting Rust panics unwind across the FFI border
/// (that aborts the process); panics surface as Java exceptions instead.
#[cfg(target_os = "android")]
pub(crate) fn guard<T: Default, F: FnOnce() -> Result<T>>(body: F) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|value| value.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            crate::android_log::error("pordo-native", &format!("panic: {message}"));
            Err(anyhow::anyhow!("原生错误 / Native panic: {message}"))
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) fn text(env: &mut JNIEnv<'_>, value: &JString<'_>) -> Result<String> {
    Ok(env.get_string(value).context("JNI string")?.into())
}

#[cfg(target_os = "android")]
pub(crate) fn json_string(env: &mut JNIEnv<'_>, value: &serde_json::Value) -> Result<jstring> {
    Ok(env.new_string(value.to_string())?.into_raw())
}

/// Binary container: 4-byte big-endian JSON length, JSON metadata, payload.
#[cfg(target_os = "android")]
pub(crate) fn container(
    env: &mut JNIEnv<'_>,
    metadata: &serde_json::Value,
    payload: &[u8],
) -> Result<jbyteArray> {
    container_parts(env, metadata, &[payload])
}

/// Copy directly into the Java array, keeping native image tiles separate.
/// No flattened payload or second full-size native container is needed.
#[cfg(target_os = "android")]
fn container_parts(
    env: &mut JNIEnv<'_>,
    metadata: &serde_json::Value,
    payloads: &[&[u8]],
) -> Result<jbyteArray> {
    let json = metadata.to_string();
    let total = payloads.iter().try_fold(4usize + json.len(), |size, part| {
        size.checked_add(part.len())
    });
    let total = i32::try_from(total.context("JNI container size overflow")?)
        .context("JNI container exceeds Java array limit")?;
    let array = env.new_byte_array(total)?;
    let header = (json.len() as u32).to_be_bytes();
    let mut offset = 0;
    for bytes in [&header[..], json.as_bytes()]
        .into_iter()
        .chain(payloads.iter().copied())
    {
        // SAFETY: jbyte is i8; the borrowed slice remains valid for the copy.
        let signed =
            unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<i8>(), bytes.len()) };
        env.set_byte_array_region(&array, offset, signed)?;
        offset += bytes.len() as i32;
    }
    Ok(array.into_raw())
}

/// Image container: RGBA tiles stacked in decode order plus their layout.
#[cfg(target_os = "android")]
pub(crate) fn image_container(
    env: &mut JNIEnv<'_>,
    image: &pordo_core::comics::cache::CachedImage,
) -> Result<jbyteArray> {
    let mut payloads = Vec::with_capacity(image.bitmap.tiles.len());
    let mut tiles = Vec::new();
    let mut offset = 0;
    for tile in &image.bitmap.tiles {
        tiles.push(serde_json::json!({
            "width": tile.width(),
            "height": tile.height(),
            "offset": offset,
        }));
        payloads.push(tile.bytes());
        offset += tile.bytes().len();
    }
    container_parts(
        env,
        &serde_json::json!({
            "width": image.bitmap.width,
            "height": image.bitmap.height,
            "tiles": tiles,
        }),
        &payloads,
    )
}

/// Install the sandbox data root (the app files directory) and hand the app
/// context to the TLS verifier. Call once before any other native method;
/// later calls keep the first root.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeInit(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    root: JString<'_>,
    context: JObject<'_>,
) {
    let result = guard(|| -> Result<()> {
        std::panic::set_hook(Box::new(|info| {
            crate::android_log::error("pordo-native", &format!("panic: {info}"));
        }));
        // Surface `log` records (platform verifier warnings carry Android's own
        // failure message); idempotent across repeated nativeInit calls.
        let _ = log::set_logger(&crate::android_log::LOGGER)
            .map(|()| log::set_max_level(log::LevelFilter::Info));
        // TLS first: a failed verifier must abort initialization so Kotlin never
        // marks the bridge ready and every later HTTPS call would panic.
        init_tls_verifier(&mut env, &context)?;
        pordo_core::paths::set_root(std::path::PathBuf::from(text(&mut env, &root)?));
        Ok(())
    });
    failure(&mut env, result);
}

/// reqwest 0.13 verifies TLS through rustls-platform-verifier, which on Android
/// resolves the system trust store from the application context and panics on
/// every later verification until it is initialized.
///
/// The verifier links jni 0.22 while this bridge uses jni 0.21; only the opaque
/// C pointers (`*mut JNIEnv`, `jobject`) cross that boundary, never the Rust
/// wrappers. Initialization stays on the verifier's own supported entry point so
/// the class loader and global references are built by it, not by hand.
///
/// Retries are safe: the verifier stores its context in a `OnceCell` that is
/// only populated on success, so a failed attempt can be attempted again from a
/// fresh `nativeInit` call.
#[cfg(target_os = "android")]
fn init_tls_verifier(env: &mut JNIEnv<'_>, context: &JObject<'_>) -> Result<()> {
    let raw_env = env.get_raw() as *mut jni22::sys::JNIEnv;
    let raw_context = context.as_raw();
    let mut unowned = unsafe { jni22::EnvUnowned::from_raw(raw_env) };
    // `with_env` borrows the JNI stack frame ART already opened for this native
    // call; the guard does not own the attachment, so the caller's thread is
    // never detached and locals are released when `nativeInit` returns.
    let outcome = unowned
        .with_env(|env22| -> std::result::Result<(), jni22::errors::Error> {
            let context22 = unsafe { jni22::objects::JObject::from_raw(env22, raw_context) };
            rustls_platform_verifier::android::init_with_env(env22, context22)
        })
        .into_outcome();
    match outcome {
        jni22::Outcome::Ok(()) => Ok(()),
        jni22::Outcome::Err(error) => Err(anyhow::anyhow!(
            "TLS 校验器初始化失败 / TLS verifier initialization failed: {error}"
        )),
        jni22::Outcome::Panic(_) => Err(anyhow::anyhow!(
            "TLS 校验器初始化崩溃 / TLS verifier initialization panicked"
        )),
    }
}
