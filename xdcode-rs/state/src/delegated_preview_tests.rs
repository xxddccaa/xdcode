//! Verify delegated preview decoding and tool identity filtering.

use super::delegated_output_preview;
use pretty_assertions::assert_eq;
use xdcode_protocol::items::FunctionCallOutputItem;
use xdcode_protocol::models::FunctionCallOutputBody;

#[test]
fn delegated_previews_unwrap_only_recognized_inputs() {
    let wrapped = "<xdcode_delegation>\n  <source_thread_id>source</source_thread_id>\n  <input>Check &lt;main&gt; &amp; &amp;lt;literal&amp;gt;</input>\n</xdcode_delegation>";
    for (namespace, name, text, expected) in [
        (
            Some("xdcode_app"),
            "create_thread",
            wrapped,
            Some("Check <main> & &lt;literal&gt;"),
        ),
        (
            Some("xdcode_tui"),
            "send_message_to_thread",
            wrapped,
            Some("Check <main> & &lt;literal&gt;"),
        ),
        (
            Some("xdcode_app"),
            "create_thread",
            "plain &lt;text&gt;",
            Some("plain &lt;text&gt;"),
        ),
        (Some("xdcode_tui"), "create_thread", "  ", None),
        (Some("other"), "create_thread", wrapped, None),
        (None, "create_thread", wrapped, None),
        (Some("xdcode_app"), "shell", wrapped, None),
        (
            Some("xdcode_app"),
            "create_thread",
            "<xdcode_delegation>incomplete",
            Some("<xdcode_delegation>incomplete"),
        ),
    ] {
        let output = FunctionCallOutputItem {
            id: "output".to_string(),
            name: name.to_string(),
            namespace: namespace.map(str::to_string),
            output: FunctionCallOutputBody::Text(text.to_string()),
        };
        assert_eq!(delegated_output_preview(&output).as_deref(), expected);
    }
}
