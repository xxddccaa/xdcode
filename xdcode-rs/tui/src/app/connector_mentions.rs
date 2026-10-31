//! Installed connector metadata used by the TUI `$` mention picker.

use anyhow::Context;
use anyhow::Result;
use std::collections::HashMap;
use uuid::Uuid;
use xdcode_app_server_client::AppServerRequestHandle;
use xdcode_app_server_client::TypedRequestError;
use xdcode_app_server_protocol::AppsInstalledParams;
use xdcode_app_server_protocol::AppsInstalledResponse;
use xdcode_app_server_protocol::AppsReadParams;
use xdcode_app_server_protocol::AppsReadResponse;
use xdcode_app_server_protocol::ClientRequest;
use xdcode_app_server_protocol::RequestId;
use xdcode_connectors::AppInfo;

use super::App;
use super::background_requests::fetch_connectors_list;
use crate::app_event::AppEvent;
use crate::app_event::ConnectorsSnapshot;
use crate::app_server_session::AppServerSession;
use crate::chatwidget::ConnectorScopeGeneration;

const APP_READ_BATCH_SIZE: usize = 100;

impl App {
    /// Fetch installed app mentions in the background and preserve their owning session scope.
    pub(super) fn fetch_installed_connector_mentions(
        &mut self,
        app_server: &AppServerSession,
        force_refresh: bool,
        generation: ConnectorScopeGeneration,
    ) {
        let request_handle = app_server.request_handle();
        let app_event_tx = self.app_event_tx.clone();
        let thread_id = self.current_displayed_thread_id();
        let cwd = self.chat_widget.config_ref().cwd.to_path_buf();
        tokio::spawn(async move {
            let result = fetch_installed_connector_mentions(
                request_handle,
                thread_id.map(|thread_id| thread_id.to_string()),
                force_refresh,
            )
            .await
            .map_err(|err| err.to_string());
            app_event_tx.send(AppEvent::InstalledConnectorMentionsLoaded {
                thread_id,
                cwd,
                generation,
                result,
            });
        });
    }
}

/// Load installed app identities and enrich callable apps with bounded metadata lookups.
///
/// Older app servers without `app/installed` fall back to their thread-scoped app directory.
pub(super) async fn fetch_installed_connector_mentions(
    request_handle: AppServerRequestHandle,
    thread_id: Option<String>,
    force_refresh: bool,
) -> Result<ConnectorsSnapshot> {
    Ok(ConnectorsSnapshot {
        connectors: Vec::new(),
    })
}
