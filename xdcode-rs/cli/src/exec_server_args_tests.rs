#![cfg(test)]

use super::*;

#[test]
fn exec_server_local_transports_remain_available() {
    for transport in ["stdio", "stdio://", "ws://127.0.0.1:8765"] {
        assert!(
            MultitoolCli::try_parse_from(["xdcode", "exec-server", "--listen", transport]).is_ok()
        );
    }
}
