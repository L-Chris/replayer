//! Desktop media metadata. Shared probing lives in the core; this wrapper adds
//! the desktop-only source preparation (encrypted adapters, torrents).
pub use pordo_core::media::*;

use std::{path::Path, sync::Arc};

pub fn probe_info(path: &str) -> Option<Arc<Info>> {
    let stop = std::sync::atomic::AtomicBool::new(false);
    let prepared = crate::media_source::prepare(path, &stop, None).ok()?;
    pordo_core::media::probe_info_at(prepared.path(), Path::new(path))
}
