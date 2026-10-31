//! Installs agent message-board tools over the shared board interface.
//! Storage, backend behavior and tool implementations live in `xdcode-message-board`.

mod extension;

pub use extension::install;

/// Storage selected by the agent consumer for opening, setup and permanent deletion.
pub fn local_board_options() -> xdcode_message_board::LocalBoardOptions {
    xdcode_message_board::LocalBoardOptions {
        storage_namespace: "agent_message_board".into(),
    }
}
