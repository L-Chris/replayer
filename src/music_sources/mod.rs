//! Music sources: shared discovery, resolution and downloads live in the
//! core; the desktop keeps the loopback streaming cache.
pub mod streaming;

pub use pordo_core::music_sources::*;

// streaming.rs and tests.rs pick these up through `use super::*`, matching the
// pre-extraction module imports.
#[allow(unused_imports)]
use anyhow::Result;
#[allow(unused_imports)]
use futures_util::future::BoxFuture;
#[allow(unused_imports)]
use std::{path::PathBuf, sync::Arc};
