//! Read-path helpers for Xdcode memories.
//!
//! This crate owns memory injection, memory citation parsing, and telemetry
//! classification for read access to the memory folder. It intentionally does
//! not depend on the memory write pipeline.

pub mod citations;
mod metrics;
pub mod usage;

use xdcode_utils_absolute_path::AbsolutePathBuf;

pub fn memory_root(xdcode_home: &AbsolutePathBuf) -> AbsolutePathBuf {
    xdcode_home.join("memories")
}
