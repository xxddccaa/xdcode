#[test]
fn plugin_arguments_are_parsed_before_startup() -> Result<(), Box<dyn std::error::Error>> {
    let program = xdcode_utils_cargo_bin::cargo_bin("xdcode-code-mode-host")?;
    for arguments in [
        ["--help", "--xdcode-ui-plugin-process"],
        ["--xdcode-ui-plugin-process", "--help"],
    ] {
        let output = xdcode_utils_process::background_command(&program)
            .args(arguments)
            .output()?;
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)?.contains("Usage:"));
    }
    for arguments in [
        ["--unknown", "--xdcode-ui-plugin-process"],
        ["--xdcode-ui-plugin-process", "--unknown"],
        ["--listen=stdio", "--xdcode-ui-plugin-process"],
        ["--xdcode-ui-plugin-process", "--listen=stdio"],
    ] {
        let output = xdcode_utils_process::background_command(&program)
            .args(arguments)
            .output()?;
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr)?.contains("error:"));
    }
    Ok(())
}
