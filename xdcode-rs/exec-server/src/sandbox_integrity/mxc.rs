//! Discover MXC dependencies separately from resolving configured policy rules.

use super::dependencies::Dependency;
use super::dependencies::IntegrityCommand;
use super::dependencies::xdcode_and_launcher_dependencies;
use std::io;
use xdcode_protocol::permissions::FileSystemSandboxPolicy;
use xdcode_sandboxing::SandboxType;
use xdcode_utils_absolute_path::AbsolutePathBuf;

pub(super) const SANDBOX_TYPE: SandboxType = SandboxType::WindowsMxc;
pub(super) const NAME: &str = "windows_mxc";

pub(super) fn critical_dependencies(
    request: &IntegrityCommand,
    _policy: &FileSystemSandboxPolicy,
    _cwd: &AbsolutePathBuf,
) -> io::Result<Vec<Dependency>> {
    Ok(xdcode_and_launcher_dependencies(request))
}

pub(super) fn prepare_policy(
    request: &IntegrityCommand,
    policy: FileSystemSandboxPolicy,
    cwd: &AbsolutePathBuf,
) -> io::Result<FileSystemSandboxPolicy> {
    let command_cwd = request.cwd.to_abs_path()?;
    xdcode_mxc_sandbox::prepare_file_system_policy(policy, &request.env, cwd, command_cwd.as_path())
        .map_err(io::Error::other)
}
