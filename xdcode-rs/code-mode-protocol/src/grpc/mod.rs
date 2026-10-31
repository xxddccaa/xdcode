#![allow(clippy::double_must_use)] // Emitted by async_trait in the generated tonic service.

#[cfg(xdcode_bazel)]
pub use code_mode_proto::xdcode::code_mode::v1::*;

#[cfg(not(xdcode_bazel))]
tonic::include_proto!("xdcode.code_mode.v1");

pub const MAX_IDENTIFIER_BYTES: usize = 256;

/// Decimal nanoseconds a completed result was buffered before a wait or terminate retrieved it.
/// Zero means immediate delivery. Hosts omit this metadata if no completed result was retrieved.
pub const RESULT_BUFFERED_FOR_NS_METADATA: &str = "xdcode-code-mode-result-buffered-for-ns";
