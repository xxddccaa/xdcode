//! Standalone executor entry point; startup and sandbox dispatch are shared with Xdcode.

#[cfg(all(
    target_os = "linux",
    target_env = "musl",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[global_allocator]
static ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() -> anyhow::Result<()> {
    xdcode_build_info::initialize!();
    xdcode_exec_server_cli::run_standalone()
}
