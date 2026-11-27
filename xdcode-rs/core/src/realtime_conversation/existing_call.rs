use super::RealtimeHandoffState;
use super::RealtimeInputChannels;
use super::RealtimeOutput;
use super::RealtimeSessionKind;
use super::RealtimeSidebandSessionInitialization;
use super::RealtimeTranscriptTailFlush;
use super::RealtimeWebrtcSidebandInputTask;
use super::spawn_webrtc_sideband_input_task;
use crate::client::ModelClient;
use async_channel::Sender;
use http::HeaderMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use xdcode_api::RealtimeEventParser;
use xdcode_api::RealtimeSessionConfig;
use xdcode_api::RealtimeTranscriptState;
use xdcode_api::RealtimeWebsocketClient;
use xdcode_api::map_api_error;
use xdcode_async_utils::OrCancelExt;
use xdcode_http_client::HttpClientFactory;
use xdcode_login::XdcodeAuth;
use xdcode_login::default_client::default_headers;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::protocol::RealtimeExistingCallAuth;

pub(super) struct ExistingCallAttachment {
    pub(super) attachment_cancellation: CancellationToken,
    pub(super) existing_call_auth: RealtimeExistingCallAuth,
    pub(super) client: RealtimeWebsocketClient,
    pub(super) model_client: ModelClient,
    pub(super) auth_context: xdcode_model_provider::ProviderAuthContext,
    pub(super) http_client_factory: HttpClientFactory,
    pub(super) sideband_base_url: String,
    pub(super) session_config: RealtimeSessionConfig,
    pub(super) call_id: String,
    pub(super) extra_headers: HeaderMap,
    pub(super) input_channels: RealtimeInputChannels,
    pub(super) events_tx: Sender<RealtimeOutput>,
    pub(super) handoff_state: RealtimeHandoffState,
    pub(super) session_kind: RealtimeSessionKind,
    pub(super) event_parser: RealtimeEventParser,
    pub(super) realtime_active: Arc<AtomicBool>,
    pub(super) transcript_tail_flush: RealtimeTranscriptTailFlush,
    pub(super) stop_token: CancellationToken,
}

pub(super) async fn attach(attachment: ExistingCallAttachment) -> XdcodeResult<JoinHandle<()>> {
    let ExistingCallAttachment {
        attachment_cancellation,
        existing_call_auth,
        client,
        model_client,
        auth_context,
        http_client_factory,
        sideband_base_url,
        session_config,
        call_id,
        extra_headers,
        input_channels,
        events_tx,
        handoff_state,
        session_kind,
        event_parser,
        realtime_active,
        transcript_tail_flush,
        stop_token,
    } = attachment;
    let (auth, sideband_headers, factory) = match existing_call_auth {
        RealtimeExistingCallAuth::ModelProvider => {
            model_client
                .realtime_sideband_auth(
                    extra_headers,
                    &sideband_base_url,
                    &auth_context,
                    http_client_factory,
                )
                .or_cancel(&attachment_cancellation)
                .await??
        }
        RealtimeExistingCallAuth::ExternalHeaders => {
            let Some(auth @ XdcodeAuth::Headers(_)) = auth_context
                .credentials
                .auth()
                .or_cancel(&attachment_cancellation)
                .await?
            else {
                return Err(XdcodeErr::InvalidRequest(
                    "external header authentication requires header-based authentication".into(),
                ));
            };
            (
                xdcode_model_provider::auth_provider_from_auth(&auth),
                extra_headers,
                http_client_factory,
            )
        }
    };
    let client = client.with_http_client_factory(factory).with_auth(auth);
    let transcript_state = RealtimeTranscriptState::default();
    let connection = client
        .connect_existing_call_sideband(
            session_config.clone(),
            &call_id,
            sideband_headers.clone(),
            default_headers(),
            transcript_state,
        )
        .or_cancel(&attachment_cancellation)
        .await?
        .map_err(map_api_error)?;

    Ok(spawn_webrtc_sideband_input_task(
        RealtimeWebrtcSidebandInputTask {
            client,
            session_config,
            call_id,
            sideband_headers,
            session_initialization: RealtimeSidebandSessionInitialization::ExistingCall {
                initial_connection: Some(connection),
            },
            input_channels,
            events_tx,
            handoff_state,
            session_kind,
            event_parser,
            realtime_active,
            transcript_tail_flush,
            stop_token,
        },
    ))
}
