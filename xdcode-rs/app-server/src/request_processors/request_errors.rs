use super::*;
use xdcode_protocol::error::XdcodeErrorDetails;

pub(super) fn environment_selection_error(err: XdcodeErr) -> JSONRPCErrorError {
    match err.details() {
        XdcodeErrorDetails::InvalidRequest(message) => invalid_request(message.clone()),
        _ => internal_error(format!("failed to validate environment selections: {err}")),
    }
}
