//! Discover MXC dependencies separately from resolving configured policy rules.

use super::dependencies::Dependency;
use super::dependencies::xdcode_and_launcher_dependencies;
use std::io;
use xdcode_protocol::permissions::FileSystemSandboxPolicy;
use xdcode_sandboxing::SandboxExecRequest;
use xdcode_sandboxing::SandboxType;
use xdcode_utils_absolute_path::AbsolutePathBuf;

pub(super) const SANDBOX_TYPE: SandboxType = SandboxType::WindowsMxc;
pub(super) const NAME: &str = "windows_mxc";

pub(super) fn critical_dependencies(
    request: &SandboxExecRequest,
    policy: &FileSystemSandboxPolicy,
    cwd: &AbsolutePathBuf,
) -> io::Result<Vec<Dependency>> {
    let command_cwd = request.cwd.to_abs_path()?;
    let mut dependencies = xdcode_and_launcher_dependencies(request);
    if !policy
        .get_unreadable_globs_with_cwd(cwd.as_path())
        .is_empty()
    {
        let path = request
            .env
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value);
        if let Ok(scanner) = which::which_in(
            xdcode_mxc_sandbox::GLOB_SCAN_PROGRAM,
            path,
            command_cwd.as_path(),
        ) {
            dependencies.push(("glob_scanner", Ok(scanner)));
        }
    }
    Ok(dependencies)
}

pub(super) fn prepare_policy(
    request: &SandboxExecRequest,
    policy: FileSystemSandboxPolicy,
    cwd: &AbsolutePathBuf,
) -> io::Result<FileSystemSandboxPolicy> {
    let command_cwd = request.cwd.to_abs_path()?;
    xdcode_mxc_sandbox::prepare_file_system_policy(policy, &request.env, cwd, command_cwd.as_path())
        .map_err(io::Error::other)
}
