//! Bridges the board extension to the selected controller, clock and active-turn services.
//!
//! The shared board library owns storage; the extension owns tools. This adapter never starts or restores
//! recipients and never queues a notification for an idle agent.

use crate::ThreadManager;
use crate::XdcodeThread;
use crate::config::Config;
use crate::context::AgentMessageBoardNotification;
use crate::context::ContextualUserFragment;
use crate::tools::MULTI_AGENT_V2_NAMESPACE_DESCRIPTION;
use chrono::DateTime;
use chrono::Utc;
use futures::future::BoxFuture;
use std::sync::Arc;
use std::sync::Weak;
use xdcode_agent_message_board_client::AccessToken;
use xdcode_agent_message_board_client::RemoteAgentMessageBoard;
use xdcode_agent_message_board_extension::local_board_options;
use xdcode_extension_api::ExtensionRegistryBuilder;
use xdcode_extension_api::ThreadStartInput;
use xdcode_features::Feature;
use xdcode_http_client::ClientRouteClass;
use xdcode_message_board::AgentMessageBoard;
use xdcode_message_board::InMemoryMessageBoards;
use xdcode_message_board::LocalAgentMessageBoard;
use xdcode_message_board::LocalBoardConnection;
use xdcode_message_board::MessageBoardHost;
use xdcode_message_board::NotificationDelivery;
use xdcode_message_board::PostPreview;
use xdcode_protocol::AgentPath;
use xdcode_protocol::SessionId;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::Result;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::error::XdcodeErrorDetails;
use xdcode_protocol::protocol::InterAgentCommunication;

/// Registers the configured board for opted-in MAv2 runtimes.
/// Training can share an in-memory board per tree, including ephemeral sessions.
pub fn install_agent_message_board(
    registry: &mut ExtensionRegistryBuilder<Config>,
    manager: Weak<ThreadManager>,
) {
    xdcode_agent_message_board_client::install_notifications(registry, |input| {
        let board = input.thread_store.get::<RemoteAgentMessageBoard>()?;
        let host = input.thread_store.get::<LocalBoardHost>()?;
        Some((
            board,
            Arc::new(LocalBoardHost {
                expected_turn_id: Some(input.turn_id.into()),
                ..host.as_ref().clone()
            }),
        ))
    });
    let in_memory_boards = Arc::new(InMemoryMessageBoards::default());
    xdcode_agent_message_board_extension::install(
        registry,
        MULTI_AGENT_V2_NAMESPACE_DESCRIPTION,
        |config: &Config| config.multi_agent_v2.tool_namespace.clone(),
        move |input: &ThreadStartInput<'_, Config>, tree, caller| {
            let config = input.config;
            let connection = input.thread_store.get::<LocalBoardConnection>();
            let in_memory = config.multi_agent_v2.message_board_in_memory;
            // Automatic boards need MAv2 and persistent storage. An explicit host
            // connection also works for isolated, ephemeral runtimes.
            if connection.is_none()
                && (!config.features.enabled(Feature::AgentMessageBoard)
                    || !config.features.enabled(Feature::MultiAgentV2)
                    || (config.ephemeral
                        && !in_memory
                        && config.multi_agent_v2.message_board_remote.is_none()))
            {
                return Box::pin(async { Ok(None) });
            }
            let in_memory_boards = Arc::clone(&in_memory_boards);
            let host = Arc::new(LocalBoardHost {
                manager: manager.clone(),
                tree,
                caller,
                expected_turn_id: None,
                connection: connection.clone(),
            });
            Box::pin(async move {
                let board: Arc<dyn AgentMessageBoard> = if let Some(connection) = connection {
                    Arc::new(connection.open(host).await?)
                } else if let Some(remote) = config.multi_agent_v2.message_board_remote.clone() {
                    let token = match remote.bearer_token_env_var {
                        Some(name) => std::env::var(name).map_err(|_| XdcodeErr::InvalidRequest(
                            "message-board credential environment variable is missing or invalid".into(),
                        ))?,
                        None => remote.bearer_token.ok_or_else(|| XdcodeErr::InvalidRequest(
                            "remote message board requires a credential".into(),
                        ))?.into_inner(),
                    };
                    let token = AccessToken::new(token)
                        .map_err(|err| XdcodeErr::InvalidRequest(err.to_string()))?;
                    let http = config
                        .http_client_factory()
                        .build_client(&remote.url, ClientRouteClass::Api)
                        .map_err(|err| XdcodeErr::Io(std::io::Error::other(err)))?;
                    input.thread_store.insert(host.as_ref().clone());
                    let board = RemoteAgentMessageBoard::new(http, &remote.url, tree, token)
                        .map_err(|err| XdcodeErr::InvalidRequest(err.to_string()))?
                        .with_clock(move |caller| {
                            let host = host.clone();
                            Box::pin(async move { host.current_time(caller).await })
                        });
                    input.thread_store.get_or_init(|| board)
                } else if in_memory {
                    Arc::new(in_memory_boards.open(tree, host).await)
                } else {
                    Arc::new(
                        LocalAgentMessageBoard::open(
                            config.sqlite_config(),
                            &local_board_options(),
                            tree,
                            host,
                        )
                        .await?,
                    )
                };
                Ok(Some(board))
            })
        },
    );
}

#[derive(Clone)]
struct LocalBoardHost {
    manager: Weak<ThreadManager>,
    tree: SessionId,
    caller: ThreadId,
    expected_turn_id: Option<String>,
    connection: Option<Arc<LocalBoardConnection>>,
}

impl LocalBoardHost {
    async fn actor(&self, caller: ThreadId) -> Result<Arc<XdcodeThread>> {
        let manager = self
            .manager
            .upgrade()
            .ok_or_else(|| XdcodeErr::ThreadNotFound(caller))?;
        if let Some(connection) = &self.connection {
            return manager.get_thread_for_board(caller, connection).await;
        }
        let actor = manager.get_thread(caller).await?;
        if actor.session.session_id() != self.tree
            || actor
                .thread_extension_data()
                .get::<LocalBoardConnection>()
                .is_some()
        {
            return Err(XdcodeErr::InvalidRequest(
                "agent belongs to another message board".into(),
            ));
        }
        Ok(actor)
    }

    async fn registered_path(&self, actor: &XdcodeThread) -> Result<AgentPath> {
        let caller = actor.session.thread_id;
        if let Some(connection) = &self.connection {
            return connection.caller_path(caller);
        }
        let path = actor
            .session_source
            .get_agent_path()
            .or_else(|| (caller == ThreadId::from(self.tree)).then(AgentPath::root))
            .ok_or_else(|| XdcodeErr::InvalidRequest("agent has no tree path".into()))?;
        let resolved = actor
            .session
            .services
            .agent_control
            .resolve(
                caller,
                actor.session_source.parent_thread_id(),
                &actor.session_source,
                path.as_str(),
            )
            .await?;
        if resolved != caller {
            return Err(XdcodeErr::InvalidRequest(
                "agent does not own its tree path".into(),
            ));
        }
        Ok(path)
    }
}

impl MessageBoardHost for LocalBoardHost {
    fn agent_path(&self, caller: ThreadId) -> BoxFuture<'_, Result<AgentPath>> {
        Box::pin(async move {
            let actor = self.actor(caller).await?;
            self.registered_path(&actor).await
        })
    }

    fn resolve_agent(&self, path: AgentPath) -> BoxFuture<'_, Result<ThreadId>> {
        Box::pin(async move {
            if self.connection.is_some() {
                let caller = path
                    .as_str()
                    .strip_prefix("/root/")
                    .and_then(|id| ThreadId::from_string(id).ok())
                    .ok_or_else(|| {
                        XdcodeErr::InvalidRequest("invalid connected board agent path".into())
                    })?;
                return self
                    .actor(caller)
                    .await
                    .map(|actor| actor.session.thread_id);
            }
            let actor = self.actor(self.caller).await?;
            actor
                .session
                .services
                .agent_control
                .resolve(
                    self.caller,
                    actor.session_source.parent_thread_id(),
                    &actor.session_source,
                    path.as_str(),
                )
                .await
        })
    }

    fn current_time(&self, caller: ThreadId) -> BoxFuture<'_, Result<DateTime<Utc>>> {
        Box::pin(async move {
            let actor = self.actor(caller).await?;
            self.registered_path(&actor).await?;
            actor
                .session
                .services
                .time_provider
                .current_time(caller)
                .await
                .map_err(|error| XdcodeErr::Io(std::io::Error::other(error)))
        })
    }

    fn notify(
        &self,
        recipient_id: ThreadId,
        post: PostPreview,
    ) -> BoxFuture<'_, Result<NotificationDelivery>> {
        Box::pin(async move {
            let recipient = match self.actor(recipient_id).await {
                Ok(thread) => thread,
                Err(error) if matches!(error.details(), XdcodeErrorDetails::ThreadNotFound(_)) => {
                    return Ok(NotificationDelivery::SkippedInactive);
                }
                Err(error) => return Err(error),
            };
            let recipient_path = self.registered_path(&recipient).await?;
            let notice = AgentMessageBoardNotification(post);
            let communication = InterAgentCommunication::new(
                notice.0.metadata.author.clone(),
                recipient_path,
                Vec::new(),
                notice.render().into_text().unwrap_or_default(),
                /*trigger_turn*/ false,
            );
            // Remote metadata is untrusted. Skip an oversized notice without closing
            // the receiver; the post remains available through the board tools.
            if self.expected_turn_id.is_some()
                && communication.content.len()
                    + communication.author.as_str().len()
                    + communication.recipient.as_str().len()
                    > 1024
            {
                return Err(XdcodeErr::InvalidRequest(
                    "remote board notice exceeds the context budget".into(),
                ));
            }
            Ok(
                if recipient
                    .session
                    .input_queue
                    .deliver_mailbox_communication_to_current_turn(
                        &recipient.session.active_turn,
                        communication,
                        self.expected_turn_id.as_deref(),
                    )
                    .await
                {
                    NotificationDelivery::Accepted
                } else {
                    NotificationDelivery::SkippedInactive
                },
            )
        })
    }
}
