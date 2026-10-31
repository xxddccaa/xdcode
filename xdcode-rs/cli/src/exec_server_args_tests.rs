use super::*;

#[test]
fn exec_server_local_transports_remain_available() {
    for transport in ["stdio", "stdio://", "ws://127.0.0.1:8765"] {
        assert!(
            MultitoolCli::try_parse_from(["xdcode", "exec-server", "--listen", transport]).is_ok()
        );
    }
}

#[test]
fn exec_server_rejects_hosted_registration() {
    for arguments in [
        vec![
            "--remote",
            "https://registry.example.com",
            "--environment-id",
            "test",
        ],
        vec!["forward", "--connect", "ws://127.0.0.1:8765"],
        vec!["--use-agent-identity-auth"],
        vec!["--remote-transport", "direct", "--aws-sigv4"],
    ] {
        assert!(
            MultitoolCli::try_parse_from(["xdcode", "exec-server"].into_iter().chain(arguments))
                .is_err()
        );
    }
}
