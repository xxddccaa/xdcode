//! Seatbelt keeps deny globs as native patterns; no snapshot scan is needed.

use super::dependencies::Dependency;
use super::dependencies::xdcode_executable;
use std::io;
use std::path::PathBuf;
use xdcode_protocol::permissions::FileSystemSandboxPolicy;
use xdcode_sandboxing::SandboxExecRequest;
use xdcode_sandboxing::SandboxType;
use xdcode_utils_absolute_path::AbsolutePathBuf;

pub(super) const SANDBOX_TYPE: SandboxType = SandboxType::MacosSeatbelt;
pub(super) const NAME: &str = "seatbelt";

pub(super) fn critical_dependencies(
    _request: &SandboxExecRequest,
    _policy: &FileSystemSandboxPolicy,
    _cwd: &AbsolutePathBuf,
) -> io::Result<Vec<Dependency>> {
    Ok(vec![
        xdcode_executable(),
        (
            "seatbelt",
            Ok(PathBuf::from(
                xdcode_sandboxing::seatbelt::MACOS_PATH_TO_SEATBELT_EXECUTABLE,
            )),
        ),
    ])
}

pub(super) fn prepare_policy(
    _request: &SandboxExecRequest,
    policy: FileSystemSandboxPolicy,
    _cwd: &AbsolutePathBuf,
) -> io::Result<FileSystemSandboxPolicy> {
    Ok(policy)
}
