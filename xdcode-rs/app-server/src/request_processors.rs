use crate::bespoke_event_handling::apply_bespoke_event_handling;
use crate::command_exec::CommandExecManager;
use crate::command_exec::StartCommandExecParams;
use crate::config_manager::ConfigManager;
use crate::error_code::INPUT_TOO_LARGE_ERROR_CODE;
use crate::error_code::invalid_params;
use crate::models::supported_models;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::RequestContext;
use crate::outgoing_message::ThreadScopedOutgoingMessageSender;
use crate::skills_watcher::SkillsWatcher;
use crate::thread_status::ThreadWatchManager;
use crate::thread_status::resolve_thread_status;
use chrono::Duration as ChronoDuration;
use chrono::SecondsFormat;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;
use tokio_util::task::TaskTracker;
use toml::Value as TomlValue;
use tracing::Instrument;
use tracing::error;
use tracing::info;
use tracing::warn;
use uuid::Uuid;
use xdcode_analytics::AnalyticsEventsClient;
use xdcode_analytics::AnalyticsJsonRpcError;
use xdcode_analytics::InputError;
use xdcode_analytics::TurnSteerRequestError;
use xdcode_app_server_protocol::Account;
use xdcode_app_server_protocol::AccountLoginCompletedNotification;
use xdcode_app_server_protocol::AccountTokenUsageDailyBucket;
use xdcode_app_server_protocol::AccountTokenUsageSummary;
use xdcode_app_server_protocol::AccountUpdatedNotification;
use xdcode_app_server_protocol::AddCreditsNudgeCreditType;
use xdcode_app_server_protocol::AddCreditsNudgeEmailStatus;
use xdcode_app_server_protocol::AdditionalContextEntry;
use xdcode_app_server_protocol::AdditionalContextKind;
use xdcode_app_server_protocol::AppListUpdatedNotification;
use xdcode_app_server_protocol::AppSummary;
use xdcode_app_server_protocol::AppTemplateSummary;
use xdcode_app_server_protocol::AppTemplateUnavailableReason;
use xdcode_app_server_protocol::AppsInstalledParams;
use xdcode_app_server_protocol::AppsInstalledResponse;
use xdcode_app_server_protocol::AppsListParams;
use xdcode_app_server_protocol::AppsListResponse;
use xdcode_app_server_protocol::AppsReadParams;
use xdcode_app_server_protocol::AppsReadResponse;
use xdcode_app_server_protocol::AskForApproval;
use xdcode_app_server_protocol::AuthMode;
use xdcode_app_server_protocol::CancelLoginAccountParams;
use xdcode_app_server_protocol::CancelLoginAccountResponse;
use xdcode_app_server_protocol::CancelLoginAccountStatus;
use xdcode_app_server_protocol::ClientInfo;
use xdcode_app_server_protocol::ClientRequest;
use xdcode_app_server_protocol::ClientResponsePayload;
use xdcode_app_server_protocol::CollaborationModeListParams;
use xdcode_app_server_protocol::CollaborationModeListResponse;
use xdcode_app_server_protocol::CommandExecParams;
use xdcode_app_server_protocol::CommandExecResizeParams;
use xdcode_app_server_protocol::CommandExecTerminateParams;
use xdcode_app_server_protocol::CommandExecWriteParams;
use xdcode_app_server_protocol::ConfigWarningNotification;
use xdcode_app_server_protocol::ConsumeAccountRateLimitResetCreditOutcome;
use xdcode_app_server_protocol::ConsumeAccountRateLimitResetCreditParams;
use xdcode_app_server_protocol::ConsumeAccountRateLimitResetCreditResponse;
use xdcode_app_server_protocol::ConversationGitInfo;
use xdcode_app_server_protocol::ConversationSummary;
use xdcode_app_server_protocol::DeprecationNoticeNotification;
use xdcode_app_server_protocol::DynamicToolFunctionSpec;
use xdcode_app_server_protocol::DynamicToolNamespaceTool;
use xdcode_app_server_protocol::DynamicToolSpec;
use xdcode_app_server_protocol::EnvironmentAddParams;
use xdcode_app_server_protocol::EnvironmentAddResponse;
use xdcode_app_server_protocol::EnvironmentInfoParams;
use xdcode_app_server_protocol::EnvironmentInfoResponse;
use xdcode_app_server_protocol::EnvironmentShellInfo;
use xdcode_app_server_protocol::EnvironmentStatusKind;
use xdcode_app_server_protocol::EnvironmentStatusParams;
use xdcode_app_server_protocol::EnvironmentStatusResponse;
use xdcode_app_server_protocol::ExperimentalFeature as ApiExperimentalFeature;
use xdcode_app_server_protocol::ExperimentalFeatureListParams;
use xdcode_app_server_protocol::ExperimentalFeatureListResponse;
use xdcode_app_server_protocol::ExperimentalFeatureStage as ApiExperimentalFeatureStage;
use xdcode_app_server_protocol::FeedbackUploadParams;
use xdcode_app_server_protocol::FeedbackUploadResponse;
use xdcode_app_server_protocol::GetAccountParams;
use xdcode_app_server_protocol::GetAccountRateLimitsResponse;
use xdcode_app_server_protocol::GetAccountResponse;
use xdcode_app_server_protocol::GetAccountTokenUsageParams;
use xdcode_app_server_protocol::GetAccountTokenUsageResponse;
use xdcode_app_server_protocol::GetAuthStatusParams;
use xdcode_app_server_protocol::GetAuthStatusResponse;
use xdcode_app_server_protocol::GetConversationSummaryParams;
use xdcode_app_server_protocol::GetConversationSummaryResponse;
use xdcode_app_server_protocol::GetWorkspaceMessagesResponse;
use xdcode_app_server_protocol::GitDiffToRemoteParams;
use xdcode_app_server_protocol::GitDiffToRemoteResponse;
use xdcode_app_server_protocol::GitInfo as ApiGitInfo;
use xdcode_app_server_protocol::HookHandlerMetadata;
use xdcode_app_server_protocol::HookMetadata;
use xdcode_app_server_protocol::HooksListParams;
use xdcode_app_server_protocol::HooksListResponse;
use xdcode_app_server_protocol::InitializeParams;
use xdcode_app_server_protocol::InitializeResponse;
use xdcode_app_server_protocol::InstalledApp;
use xdcode_app_server_protocol::JSONRPCErrorError;
use xdcode_app_server_protocol::ListMcpServerStatusParams;
use xdcode_app_server_protocol::ListMcpServerStatusResponse;
use xdcode_app_server_protocol::LoginAccountParams;
use xdcode_app_server_protocol::LoginAccountResponse;
use xdcode_app_server_protocol::LoginApiKeyParams;
use xdcode_app_server_protocol::LoginAppBrand;
use xdcode_app_server_protocol::LogoutAccountResponse;
use xdcode_app_server_protocol::MarketplaceAddParams;
use xdcode_app_server_protocol::MarketplaceAddResponse;
use xdcode_app_server_protocol::MarketplaceInterface;
use xdcode_app_server_protocol::MarketplaceRemoveParams;
use xdcode_app_server_protocol::MarketplaceRemoveResponse;
use xdcode_app_server_protocol::MarketplaceUpgradeErrorInfo;
use xdcode_app_server_protocol::MarketplaceUpgradeParams;
use xdcode_app_server_protocol::MarketplaceUpgradeResponse;
use xdcode_app_server_protocol::McpResourceReadParams;
use xdcode_app_server_protocol::McpResourceReadResponse;
use xdcode_app_server_protocol::McpServerOauthClientRegistration;
use xdcode_app_server_protocol::McpServerOauthLoginCompletedNotification;
use xdcode_app_server_protocol::McpServerOauthLoginParams;
use xdcode_app_server_protocol::McpServerOauthLoginResponse;
use xdcode_app_server_protocol::McpServerRefreshResponse;
use xdcode_app_server_protocol::McpServerStatus;
use xdcode_app_server_protocol::McpServerStatusDetail;
use xdcode_app_server_protocol::McpServerToolCallParams;
use xdcode_app_server_protocol::McpServerToolCallResponse;
use xdcode_app_server_protocol::MemoryResetResponse;
use xdcode_app_server_protocol::MockExperimentalMethodParams;
use xdcode_app_server_protocol::MockExperimentalMethodResponse;
use xdcode_app_server_protocol::ModelListParams;
use xdcode_app_server_protocol::ModelListResponse;
use xdcode_app_server_protocol::PermissionProfileListParams;
use xdcode_app_server_protocol::PermissionProfileListResponse;
use xdcode_app_server_protocol::PermissionProfileSummary;
use xdcode_app_server_protocol::PluginDetail;
use xdcode_app_server_protocol::PluginInstallParams;
use xdcode_app_server_protocol::PluginInstallResponse;
use xdcode_app_server_protocol::PluginInstalledParams;
use xdcode_app_server_protocol::PluginInstalledResponse;
use xdcode_app_server_protocol::PluginInterface;
use xdcode_app_server_protocol::PluginListMarketplaceKind;
use xdcode_app_server_protocol::PluginListParams;
use xdcode_app_server_protocol::PluginListResponse;
use xdcode_app_server_protocol::PluginMarketplaceEntry;
use xdcode_app_server_protocol::PluginReadParams;
use xdcode_app_server_protocol::PluginReadResponse;
use xdcode_app_server_protocol::PluginShareCheckoutParams;
use xdcode_app_server_protocol::PluginShareCheckoutResponse;
use xdcode_app_server_protocol::PluginShareContext;
use xdcode_app_server_protocol::PluginShareDeleteParams;
use xdcode_app_server_protocol::PluginShareDeleteResponse;
use xdcode_app_server_protocol::PluginShareDiscoverability;
use xdcode_app_server_protocol::PluginShareListItem;
use xdcode_app_server_protocol::PluginShareListParams;
use xdcode_app_server_protocol::PluginShareListResponse;
use xdcode_app_server_protocol::PluginSharePrincipal;
use xdcode_app_server_protocol::PluginSharePrincipalType;
use xdcode_app_server_protocol::PluginShareSaveParams;
use xdcode_app_server_protocol::PluginShareSaveResponse;
use xdcode_app_server_protocol::PluginShareTarget;
use xdcode_app_server_protocol::PluginShareUpdateDiscoverability;
use xdcode_app_server_protocol::PluginShareUpdateTargetsParams;
use xdcode_app_server_protocol::PluginShareUpdateTargetsResponse;
use xdcode_app_server_protocol::PluginSkillReadParams;
use xdcode_app_server_protocol::PluginSkillReadResponse;
use xdcode_app_server_protocol::PluginSource;
use xdcode_app_server_protocol::PluginSummary;
use xdcode_app_server_protocol::PluginUninstallParams;
use xdcode_app_server_protocol::PluginUninstallResponse;
use xdcode_app_server_protocol::RateLimitResetCredit;
use xdcode_app_server_protocol::RateLimitResetCreditStatus;
use xdcode_app_server_protocol::RateLimitResetCreditsSummary;
use xdcode_app_server_protocol::RateLimitResetType;
use xdcode_app_server_protocol::RequestId;
use xdcode_app_server_protocol::ReviewDelivery as ApiReviewDelivery;
use xdcode_app_server_protocol::ReviewStartParams;
use xdcode_app_server_protocol::ReviewStartResponse;
use xdcode_app_server_protocol::ReviewTarget as ApiReviewTarget;
use xdcode_app_server_protocol::SandboxMode;
use xdcode_app_server_protocol::SendAddCreditsNudgeEmailParams;
use xdcode_app_server_protocol::SendAddCreditsNudgeEmailResponse;
use xdcode_app_server_protocol::ServerNotification;
use xdcode_app_server_protocol::ServerRequestResolvedNotification;
use xdcode_app_server_protocol::SkillSummary;
use xdcode_app_server_protocol::SkillsConfigWriteParams;
use xdcode_app_server_protocol::SkillsConfigWriteResponse;
use xdcode_app_server_protocol::SkillsExtraRootsSetParams;
use xdcode_app_server_protocol::SkillsExtraRootsSetResponse;
use xdcode_app_server_protocol::SkillsListParams;
use xdcode_app_server_protocol::SkillsListResponse;
use xdcode_app_server_protocol::SortDirection;
use xdcode_app_server_protocol::Thread;
use xdcode_app_server_protocol::ThreadApproveGuardianDeniedActionParams;
use xdcode_app_server_protocol::ThreadApproveGuardianDeniedActionResponse;
use xdcode_app_server_protocol::ThreadArchiveParams;
use xdcode_app_server_protocol::ThreadArchiveResponse;
use xdcode_app_server_protocol::ThreadArchivedNotification;
use xdcode_app_server_protocol::ThreadBackgroundTerminal;
use xdcode_app_server_protocol::ThreadBackgroundTerminalsCleanParams;
use xdcode_app_server_protocol::ThreadBackgroundTerminalsCleanResponse;
use xdcode_app_server_protocol::ThreadBackgroundTerminalsListParams;
use xdcode_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use xdcode_app_server_protocol::ThreadBackgroundTerminalsTerminateParams;
use xdcode_app_server_protocol::ThreadBackgroundTerminalsTerminateResponse;
use xdcode_app_server_protocol::ThreadClosedNotification;
use xdcode_app_server_protocol::ThreadCompactStartParams;
use xdcode_app_server_protocol::ThreadCompactStartResponse;
use xdcode_app_server_protocol::ThreadDecrementElicitationParams;
use xdcode_app_server_protocol::ThreadDecrementElicitationResponse;
use xdcode_app_server_protocol::ThreadDeleteParams;
use xdcode_app_server_protocol::ThreadDeleteResponse;
use xdcode_app_server_protocol::ThreadDeletedNotification;
use xdcode_app_server_protocol::ThreadForkParams;
use xdcode_app_server_protocol::ThreadForkResponse;
use xdcode_app_server_protocol::ThreadGoal;
use xdcode_app_server_protocol::ThreadGoalClearParams;
use xdcode_app_server_protocol::ThreadGoalClearResponse;
use xdcode_app_server_protocol::ThreadGoalClearedNotification;
use xdcode_app_server_protocol::ThreadGoalGetParams;
use xdcode_app_server_protocol::ThreadGoalGetResponse;
use xdcode_app_server_protocol::ThreadGoalSetParams;
use xdcode_app_server_protocol::ThreadGoalSetResponse;
use xdcode_app_server_protocol::ThreadGoalStatus;
use xdcode_app_server_protocol::ThreadGoalUpdatedNotification;
use xdcode_app_server_protocol::ThreadHistoryBuilder;
#[cfg(test)]
use xdcode_app_server_protocol::ThreadHistoryMode;
use xdcode_app_server_protocol::ThreadIncrementElicitationParams;
use xdcode_app_server_protocol::ThreadIncrementElicitationResponse;
use xdcode_app_server_protocol::ThreadInjectItemsParams;
use xdcode_app_server_protocol::ThreadInjectItemsResponse;
use xdcode_app_server_protocol::ThreadItem;
use xdcode_app_server_protocol::ThreadItemEntry;
use xdcode_app_server_protocol::ThreadItemsListParams;
use xdcode_app_server_protocol::ThreadItemsListResponse;
use xdcode_app_server_protocol::ThreadItemsReadParams;
use xdcode_app_server_protocol::ThreadItemsReadResponse;
use xdcode_app_server_protocol::ThreadListCwdFilter;
use xdcode_app_server_protocol::ThreadListParams;
use xdcode_app_server_protocol::ThreadListResponse;
use xdcode_app_server_protocol::ThreadLoadedListParams;
use xdcode_app_server_protocol::ThreadLoadedListResponse;
use xdcode_app_server_protocol::ThreadMemoryModeSetParams;
use xdcode_app_server_protocol::ThreadMemoryModeSetResponse;
use xdcode_app_server_protocol::ThreadMetadataGitInfoUpdateParams;
use xdcode_app_server_protocol::ThreadMetadataUpdateParams;
use xdcode_app_server_protocol::ThreadMetadataUpdateResponse;
use xdcode_app_server_protocol::ThreadNameUpdatedNotification;
use xdcode_app_server_protocol::ThreadProjectUpdatedNotification;
use xdcode_app_server_protocol::ThreadReadParams;
use xdcode_app_server_protocol::ThreadReadResponse;
use xdcode_app_server_protocol::ThreadRealtimeAppendAudioParams;
use xdcode_app_server_protocol::ThreadRealtimeAppendAudioResponse;
use xdcode_app_server_protocol::ThreadRealtimeAppendSpeechParams;
use xdcode_app_server_protocol::ThreadRealtimeAppendSpeechResponse;
use xdcode_app_server_protocol::ThreadRealtimeAppendTextParams;
use xdcode_app_server_protocol::ThreadRealtimeAppendTextResponse;
use xdcode_app_server_protocol::ThreadRealtimeListVoicesResponse;
use xdcode_app_server_protocol::ThreadRealtimeStartParams;
use xdcode_app_server_protocol::ThreadRealtimeStartResponse;
use xdcode_app_server_protocol::ThreadRealtimeStartTransport;
use xdcode_app_server_protocol::ThreadRealtimeStopParams;
use xdcode_app_server_protocol::ThreadRealtimeStopResponse;
use xdcode_app_server_protocol::ThreadResumeInitialTurnsPageParams;
use xdcode_app_server_protocol::ThreadResumeParams;
use xdcode_app_server_protocol::ThreadResumeResponse;
use xdcode_app_server_protocol::ThreadSearchOccurrence;
use xdcode_app_server_protocol::ThreadSearchOccurrencesParams;
use xdcode_app_server_protocol::ThreadSearchOccurrencesResponse;
use xdcode_app_server_protocol::ThreadSearchParams;
use xdcode_app_server_protocol::ThreadSearchResponse;
use xdcode_app_server_protocol::ThreadSearchResult;
use xdcode_app_server_protocol::ThreadSearchSortKey;
use xdcode_app_server_protocol::ThreadSearchTextRange;
use xdcode_app_server_protocol::ThreadSetNameParams;
use xdcode_app_server_protocol::ThreadSetNameResponse;
use xdcode_app_server_protocol::ThreadSettings;
use xdcode_app_server_protocol::ThreadSettingsUpdateParams;
use xdcode_app_server_protocol::ThreadSettingsUpdateResponse;
use xdcode_app_server_protocol::ThreadShellCommandParams;
use xdcode_app_server_protocol::ThreadShellCommandResponse;
use xdcode_app_server_protocol::ThreadSortKey;
use xdcode_app_server_protocol::ThreadSourceKind;
use xdcode_app_server_protocol::ThreadStartParams;
use xdcode_app_server_protocol::ThreadStartResponse;
use xdcode_app_server_protocol::ThreadStartedNotification;
use xdcode_app_server_protocol::ThreadStatus;
use xdcode_app_server_protocol::ThreadTimelineListParams;
use xdcode_app_server_protocol::ThreadTimelineListResponse;
use xdcode_app_server_protocol::ThreadTurnsListParams;
use xdcode_app_server_protocol::ThreadTurnsListResponse;
use xdcode_app_server_protocol::ThreadUnarchiveParams;
use xdcode_app_server_protocol::ThreadUnarchiveResponse;
use xdcode_app_server_protocol::ThreadUnarchivedNotification;
use xdcode_app_server_protocol::ThreadUnsubscribeParams;
use xdcode_app_server_protocol::ThreadUnsubscribeResponse;
use xdcode_app_server_protocol::ThreadUnsubscribeStatus;
use xdcode_app_server_protocol::Turn;
use xdcode_app_server_protocol::TurnEnvironmentParams;
use xdcode_app_server_protocol::TurnError;
use xdcode_app_server_protocol::TurnInterruptParams;
use xdcode_app_server_protocol::TurnInterruptResponse;
use xdcode_app_server_protocol::TurnItemsView;
use xdcode_app_server_protocol::TurnSettingsUpdateParams;
use xdcode_app_server_protocol::TurnSettingsUpdateResponse;
use xdcode_app_server_protocol::TurnSettingsUpdateStatus;
use xdcode_app_server_protocol::TurnStartParams;
use xdcode_app_server_protocol::TurnStartResponse;
use xdcode_app_server_protocol::TurnStatus;
use xdcode_app_server_protocol::TurnSteerParams;
use xdcode_app_server_protocol::TurnSteerResponse;
use xdcode_app_server_protocol::UserInput as V2UserInput;
use xdcode_app_server_protocol::WindowsSandboxReadiness;
use xdcode_app_server_protocol::WindowsSandboxReadinessResponse;
use xdcode_app_server_protocol::WindowsSandboxSetupCompletedNotification;
use xdcode_app_server_protocol::WindowsSandboxSetupMode;
use xdcode_app_server_protocol::WindowsSandboxSetupStartParams;
use xdcode_app_server_protocol::WindowsSandboxSetupStartResponse;
use xdcode_app_server_protocol::WorkspaceMessage;
use xdcode_app_server_protocol::WorkspaceMessageType;
use xdcode_app_server_protocol::XdcodeErrorInfo;
use xdcode_arg0::Arg0DispatchPaths;
use xdcode_backend_client::AddCreditsNudgeCreditType as BackendAddCreditsNudgeCreditType;
use xdcode_backend_client::Client as BackendClient;
use xdcode_backend_client::ConsumeRateLimitResetCreditCode as BackendConsumeRateLimitResetCreditCode;
use xdcode_backend_client::RateLimitResetCreditDetails as BackendRateLimitResetCreditDetails;
use xdcode_backend_client::RateLimitResetCreditsDetails as BackendRateLimitResetCreditsDetails;
use xdcode_backend_client::RequestError as BackendRequestError;
use xdcode_backend_client::TokenUsageProfile;
use xdcode_backend_client::XdcodeWorkspaceMessage as BackendWorkspaceMessage;
use xdcode_backend_client::XdcodeWorkspaceMessageType as BackendWorkspaceMessageType;
use xdcode_backend_client::XdcodeWorkspaceMessagesResponse as BackendWorkspaceMessagesResponse;
use xdcode_chatgpt::connectors;
use xdcode_config::CloudConfigBundleLoadError;
use xdcode_config::CloudConfigBundleLoadErrorCode;
use xdcode_config::ConfigLayerStack;
use xdcode_config::loader::project_trust_key;
use xdcode_config::types::McpServerTransportConfig;
use xdcode_connectors::AppInfo;
use xdcode_core::ForkSnapshot;
use xdcode_core::McpManager;
use xdcode_core::NewThread;
use xdcode_core::NotSubmittedReason;
#[cfg(test)]
use xdcode_core::SessionMeta;
use xdcode_core::StartThreadOptions;
use xdcode_core::SteerSubmission;
use xdcode_core::ThreadConfigSnapshot;
use xdcode_core::ThreadManager;
use xdcode_core::TurnInput;
use xdcode_core::TurnInputRequest;
use xdcode_core::TurnInputSubmission;
use xdcode_core::TurnStartOptions;
use xdcode_core::XdcodeThread;
use xdcode_core::XdcodeThreadSettingsOverrides;
use xdcode_core::config::Config;
use xdcode_core::config::ConfigOverrides;
use xdcode_core::config::NetworkProxyAuditMetadata;
use xdcode_core::config::edit::ConfigEdit;
use xdcode_core::config::edit::ConfigEditsBuilder;
use xdcode_core::connectors::AccessibleConnectorsStatus;
use xdcode_core::exec::ExecCapturePolicy;
use xdcode_core::exec::ExecExpiration;
use xdcode_core::exec::ExecParams;
use xdcode_core::exec_env::create_env;
use xdcode_core::path_utils;
#[cfg(test)]
use xdcode_core::read_head_for_summary;
use xdcode_core::sandboxing::SandboxPermissions;
use xdcode_core::truncate_rollout_after_turn_id;
use xdcode_core::truncate_rollout_before_turn_id;
use xdcode_core::validate_environment_ids_and_cwds;
use xdcode_core::windows_sandbox::WindowsSandboxLevelExt;
use xdcode_core::windows_sandbox::WindowsSandboxSetupMode as CoreWindowsSandboxSetupMode;
use xdcode_core::windows_sandbox::WindowsSandboxSetupRequest;
use xdcode_core::windows_sandbox::sandbox_setup_is_complete;
use xdcode_core_plugins::PluginInstallError as CorePluginInstallError;
use xdcode_core_plugins::PluginInstallRequest;
use xdcode_core_plugins::PluginReadRequest;
use xdcode_core_plugins::PluginUninstallError as CorePluginUninstallError;
use xdcode_core_plugins::PluginsManager;
use xdcode_core_plugins::loader::load_plugin_apps;
use xdcode_core_plugins::manifest::PluginManifestInterface;
use xdcode_core_plugins::marketplace::MarketplaceError;
use xdcode_core_plugins::marketplace::MarketplacePluginSource;
use xdcode_core_plugins::marketplace_add::MarketplaceAddError;
use xdcode_core_plugins::marketplace_add::MarketplaceAddRequest;
use xdcode_core_plugins::marketplace_add::add_marketplace as add_marketplace_to_xdcode_home;
use xdcode_core_plugins::marketplace_remove::MarketplaceRemoveError;
use xdcode_core_plugins::marketplace_remove::MarketplaceRemoveRequest as CoreMarketplaceRemoveRequest;
use xdcode_core_plugins::marketplace_remove::remove_marketplace;
use xdcode_core_plugins::remote::RemoteMarketplace;
use xdcode_core_plugins::remote::RemoteMarketplaceSource;
use xdcode_core_plugins::remote::RemotePluginCatalogError;
use xdcode_core_plugins::remote::RemotePluginDetail as RemoteCatalogPluginDetail;
use xdcode_core_plugins::remote::RemotePluginServiceConfig;
use xdcode_core_plugins::remote::RemotePluginShareContext as RemoteCatalogPluginShareContext;
use xdcode_core_plugins::remote::RemotePluginShareSummary as RemoteCatalogPluginShareSummary;
use xdcode_core_plugins::remote::RemotePluginSummary as RemoteCatalogPluginSummary;
use xdcode_exec_server::EnvironmentManager;
use xdcode_exec_server::EnvironmentObservedStatus;
use xdcode_exec_server::LOCAL_ENVIRONMENT_ID;
use xdcode_exec_server::LOCAL_FS;
use xdcode_features::FEATURES;
use xdcode_features::Feature;
use xdcode_features::Stage;
use xdcode_feedback::FeedbackAttachmentPath;
use xdcode_feedback::FeedbackUploadOptions;
use xdcode_feedback::XdcodeFeedback;
use xdcode_git_utils::git_diff_to_remote;
use xdcode_git_utils::resolve_root_git_project_for_trust;
use xdcode_login::AuthManager;
use xdcode_login::LoginSuccessPage;
use xdcode_login::LoginSuccessPageBrand;
use xdcode_login::ServerOptions as LoginServerOptions;
use xdcode_login::ShutdownHandle;
use xdcode_login::XDCODE_OPEN_APP_URL;
use xdcode_login::XdcodeAuth;
use xdcode_login::complete_device_code_login;
use xdcode_login::login_with_api_key;
use xdcode_login::login_with_bedrock_api_key;
use xdcode_login::oauth_client_id;
use xdcode_login::request_device_code;
use xdcode_login::run_login_server;
use xdcode_mcp::McpRuntimeContext;
use xdcode_mcp::McpServerStatusSnapshot;
use xdcode_mcp::McpSnapshotDetail;
use xdcode_mcp::collect_mcp_server_status_snapshot_with_detail;
use xdcode_mcp::discover_supported_scopes;
use xdcode_mcp::read_mcp_resource as read_mcp_resource_without_thread;
use xdcode_mcp::resolve_oauth_scopes;
use xdcode_memories_write::clear_memory_roots_contents;
use xdcode_model_provider::create_model_provider;
use xdcode_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets;
use xdcode_protocol::ThreadId;
use xdcode_protocol::config_types::CollaborationMode;
use xdcode_protocol::config_types::ForcedLoginMethod;
use xdcode_protocol::config_types::Personality;
use xdcode_protocol::config_types::ReasoningSummary;
use xdcode_protocol::config_types::TrustLevel;
use xdcode_protocol::config_types::WindowsSandboxLevel;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErr;
#[cfg(test)]
use xdcode_protocol::items::TurnItem;
use xdcode_protocol::model_types::ReasoningEffort;
use xdcode_protocol::models::ResponseItem;
use xdcode_protocol::protocol::AgentStatus;
use xdcode_protocol::protocol::ConversationAudioParams;
use xdcode_protocol::protocol::ConversationSpeechParams;
use xdcode_protocol::protocol::ConversationStartParams;
use xdcode_protocol::protocol::ConversationStartTransport;
use xdcode_protocol::protocol::ConversationTextParams;
use xdcode_protocol::protocol::EnvironmentConfigState;
use xdcode_protocol::protocol::EventMsg;
#[cfg(test)]
use xdcode_protocol::protocol::GitInfo as CoreGitInfo;
use xdcode_protocol::protocol::McpAuthStatus as CoreMcpAuthStatus;
use xdcode_protocol::protocol::Op;
use xdcode_protocol::protocol::RealtimeVoicesList;
use xdcode_protocol::protocol::ReviewDelivery as CoreReviewDelivery;
use xdcode_protocol::protocol::ReviewRequest;
use xdcode_protocol::protocol::ReviewTarget as CoreReviewTarget;
use xdcode_protocol::protocol::SessionConfiguredEvent;
#[cfg(test)]
use xdcode_protocol::protocol::SessionMetaLine;
use xdcode_protocol::protocol::TurnEnvironmentRequest;
use xdcode_protocol::protocol::W3cTraceContext;
use xdcode_protocol::protocol::strip_user_message_prefix;
use xdcode_protocol::user_input::MAX_USER_INPUT_TEXT_CHARS;
use xdcode_protocol::user_input::UserInput as CoreInputItem;
use xdcode_rmcp_client::McpOAuthClientRegistration;
use xdcode_rmcp_client::StreamableHttpRedirectMode;
use xdcode_rmcp_client::perform_oauth_login_return_url;
use xdcode_rollout::InitialHistory;
use xdcode_rollout::ResumedHistory;
use xdcode_rollout::RolloutItem;
use xdcode_rollout::persisted_rollout_item;
use xdcode_rollout::state_db::StateDbHandle;
use xdcode_rollout::state_db::reconcile_rollout;
use xdcode_state::ThreadMetadata;
use xdcode_state::log_db::LogDbLayer;
use xdcode_thread_store::ArchiveThreadParams as StoreArchiveThreadParams;
use xdcode_thread_store::ArchiveThreadsParams as StoreArchiveThreadsParams;
use xdcode_thread_store::ClearableField as StoreClearableField;
use xdcode_thread_store::DeleteThreadsParams as StoreDeleteThreadsParams;
use xdcode_thread_store::GitInfoPatch as StoreGitInfoPatch;
use xdcode_thread_store::ItemSortKey as StoreItemSortKey;
use xdcode_thread_store::ListItemsParams as StoreListItemsParams;
use xdcode_thread_store::ListItemsPosition as StoreListItemsPosition;
use xdcode_thread_store::ListThreadsParams as StoreListThreadsParams;
use xdcode_thread_store::ListTimelineParams as StoreListTimelineParams;
use xdcode_thread_store::ListTurnsParams as StoreListTurnsParams;
use xdcode_thread_store::LoadThreadHistoryParams as StoreLoadThreadHistoryParams;
use xdcode_thread_store::LocalThreadStore;
use xdcode_thread_store::ReadThreadByRolloutPathParams as StoreReadThreadByRolloutPathParams;
use xdcode_thread_store::ReadThreadParams as StoreReadThreadParams;
use xdcode_thread_store::SearchThreadOccurrencesParams as StoreSearchThreadOccurrencesParams;
use xdcode_thread_store::SearchThreadsParams as StoreSearchThreadsParams;
use xdcode_thread_store::SortDirection as StoreSortDirection;
use xdcode_thread_store::StoredThread;
use xdcode_thread_store::StoredTurn;
use xdcode_thread_store::StoredTurnItemsView;
use xdcode_thread_store::StoredTurnStatus;
use xdcode_thread_store::ThreadMetadataPatch as StoreThreadMetadataPatch;
use xdcode_thread_store::ThreadRelationFilter as StoreThreadRelationFilter;
use xdcode_thread_store::ThreadSortKey as StoreThreadSortKey;
use xdcode_thread_store::ThreadStore;
use xdcode_thread_store::ThreadStoreError;
use xdcode_utils_absolute_path::AbsolutePathBuf;
use xdcode_utils_pty::DEFAULT_OUTPUT_BYTES_CAP;

#[cfg(test)]
use xdcode_app_server_protocol::ServerRequest;

pub(crate) mod account_processor;
mod apps_processor;
mod bedrock_auth;
mod catalog_processor;
mod command_exec_processor;
pub(crate) mod config_processor;
#[cfg(test)]
#[path = "request_processors/config_reload_tests.rs"]
mod config_reload_tests;
mod diagnostics;
mod environment_processor;
mod feedback_doctor_report;
mod feedback_processor;
mod feedback_rollout_history;
mod feedback_thread_index;
mod fs_processor;
mod git_processor;
mod initialize_processor;
mod marketplace_processor;
mod mcp_event_stream;
mod mcp_processor;
mod memory_status;
mod persisted_resume_settings;
mod plugins;
mod process_exec_processor;
mod projects;
mod remote_control_processor;
mod rollout;
mod search;
mod thread_attachments;
mod thread_enrichment;
mod thread_fork_goal;
mod thread_input;
mod thread_memory;
mod thread_processor;
mod thread_queue_processor;
mod thread_read_state;
mod thread_sections;
mod token_usage_replay;
mod turn_processor;
mod windows_sandbox_processor;

pub(crate) use account_processor::AccountRequestProcessor;
pub(crate) use apps_processor::AppsRequestProcessor;
pub(crate) use catalog_processor::CatalogRequestProcessor;
pub(crate) use command_exec_processor::CommandExecRequestProcessor;
pub(crate) use config_processor::ConfigRequestProcessor;
pub(crate) use diagnostics::read_server_diagnostics;
pub(crate) use environment_processor::EnvironmentRequestProcessor;
pub(crate) use feedback_processor::FeedbackRequestProcessor;
pub(crate) use fs_processor::FsRequestProcessor;
pub(crate) use git_processor::GitRequestProcessor;
pub(crate) use initialize_processor::InitializeRequestProcessor;
pub(crate) use marketplace_processor::MarketplaceRequestProcessor;
pub(crate) use mcp_event_stream::McpEventStreamReady;
pub(crate) use mcp_event_stream::McpEventStreams;
pub(crate) use mcp_processor::McpRequestProcessor;
pub(crate) use plugins::PluginRequestProcessor;
pub(crate) use process_exec_processor::ProcessExecRequestProcessor;
pub(crate) use projects::ProjectRequestProcessor;
pub(crate) use remote_control_processor::RemoteControlRequestProcessor;
pub(crate) use search::SearchRequestProcessor;
pub(crate) use thread_goal_processor::ThreadGoalRequestProcessor;
pub(crate) use thread_processor::ThreadRequestProcessor;
pub(crate) use thread_processor::ThreadResumeTarget;
pub(crate) use thread_queue_processor::ThreadQueueRequestProcessor;
pub(crate) use turn_processor::TurnRequestProcessor;
pub(crate) use windows_sandbox_processor::WindowsSandboxRequestProcessor;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::filters::compute_source_filters;
use crate::filters::source_kind_matches;
use crate::thread_state::ConnectionCapabilities;
use crate::thread_state::PendingThreadUnloads;
use crate::thread_state::ThreadListenerCommand;
use crate::thread_state::ThreadState;
use crate::thread_state::ThreadStateManager;
use token_usage_replay::restored_token_usage_turn_id;
use token_usage_replay::send_thread_token_usage_update_to_connection;

pub(crate) fn apply_live_thread_settings(
    thread: &mut Thread,
    config_snapshot: &ThreadConfigSnapshot,
) {
    thread.model = Some(config_snapshot.model.clone());
    thread.reasoning_effort = config_snapshot.reasoning_effort.clone();
    thread.environments = Some(
        config_snapshot
            .environment_selections()
            .iter()
            .map(Into::into)
            .collect(),
    );
}

fn resolve_request_cwd(cwd: Option<PathBuf>) -> Result<Option<AbsolutePathBuf>, JSONRPCErrorError> {
    cwd.map(|cwd| {
        AbsolutePathBuf::relative_to_current_dir(path_utils::normalize_for_native_workdir(cwd))
            .map_err(|err| invalid_request(format!("invalid cwd: {err}")))
    })
    .transpose()
}

fn resolve_turn_environment_requests(
    thread_manager: &ThreadManager,
    environments: Option<Vec<TurnEnvironmentParams>>,
) -> Result<Option<Vec<TurnEnvironmentRequest>>, JSONRPCErrorError> {
    let Some(environments) = environments else {
        return Ok(None);
    };
    let mut requests = Vec::with_capacity(environments.len());
    for environment in environments {
        let environment_id = environment.environment_id;
        let cwd = environment
            .cwd
            .to_inferred_path_uri()
            .ok_or_else(|| {
                invalid_request(format!(
                    "invalid cwd for environment `{environment_id}`: path `{}` does not use absolute POSIX or Windows path syntax",
                    environment.cwd
                ))
            })?;
        let workspace_roots = environment
            .runtime_workspace_roots
            .map(|roots| {
                let mut resolved_roots = Vec::new();
                for root in roots {
                    let root = root.to_inferred_path_uri().ok_or_else(|| {
                        invalid_request(format!(
                            "invalid runtime workspace root for environment `{environment_id}`: path `{root}` does not use absolute POSIX or Windows path syntax"
                        ))
                    })?;
                    if !resolved_roots.contains(&root) {
                        resolved_roots.push(root);
                    }
                }
                Ok::<_, JSONRPCErrorError>(resolved_roots)
            })
            .transpose()?
            .unwrap_or_else(|| vec![cwd.clone()]);
        requests.push(TurnEnvironmentRequest {
            environment_id,
            cwd,
            workspace_roots,
            config: EnvironmentConfigState::FromThread,
        });
    }
    validate_environment_ids_and_cwds(
        &thread_manager.environment_manager(),
        requests
            .iter()
            .map(|request| (request.environment_id.as_str(), &request.cwd)),
    )
    .map_err(environment_selection_error)?;
    Ok(Some(requests))
}

fn resolve_runtime_workspace_roots(workspace_roots: Vec<AbsolutePathBuf>) -> Vec<AbsolutePathBuf> {
    let mut resolved_roots = Vec::new();
    for root in workspace_roots {
        if !resolved_roots.iter().any(|existing| existing == &root) {
            resolved_roots.push(root);
        }
    }
    resolved_roots
}

pub(crate) mod config_errors;
mod request_errors;
mod thread_delete;
mod thread_goal_processor;
mod thread_lifecycle;
mod thread_resume_redaction;
mod thread_summary;

use self::config_errors::*;
use self::request_errors::*;
use self::thread_goal_processor::api_thread_goal_from_state;
use self::thread_lifecycle::*;
use self::thread_resume_redaction::*;
use self::thread_summary::*;

pub(crate) use self::thread_lifecycle::populate_thread_turns_from_history;
pub(crate) use self::thread_processor::thread_from_stored_thread;
#[cfg(test)]
pub(crate) use self::thread_summary::read_summary_from_rollout;
#[cfg(test)]
pub(crate) use self::thread_summary::summary_to_thread;
pub(crate) use self::thread_summary::thread_settings_from_config_snapshot;

pub(crate) fn build_legacy_api_turns_from_rollout_items(items: &[RolloutItem]) -> Vec<Turn> {
    let mut builder = ThreadHistoryBuilder::new();
    for item in items {
        if let Some(item) =
            persisted_rollout_item(item, xdcode_protocol::protocol::ThreadHistoryMode::Legacy)
        {
            builder.handle_rollout_item(item.as_ref());
        }
    }
    builder.finish()
}
