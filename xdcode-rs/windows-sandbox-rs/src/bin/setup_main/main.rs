#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    xdcode_windows_sandbox::setup_helper_main()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("xdcode-windows-sandbox-setup is Windows-only");
}
