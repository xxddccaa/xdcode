//! Validate a trusted host's policy snapshot against the destination's managed requirements.

use super::ExecPolicyManager;
use super::policy_files;
use std::sync::Arc;
use xdcode_config::ConfigLayerStack;
use xdcode_execpolicy::Policy;
use xdcode_execpolicy::RequirementsExecPolicy;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErr;

/// Point-in-time session policy and the managed requirements it was loaded with.
/// A trusted host transports these together and reapplies them on cold resume.
pub(crate) struct InheritedExecPolicy {
    pub(crate) policy: Arc<Policy>,
    pub(crate) source_requirements: Option<Arc<RequirementsExecPolicy>>,
}

impl ExecPolicyManager {
    pub(crate) async fn from_inherited(
        snapshot: &InheritedExecPolicy,
        stack: &ConfigLayerStack,
    ) -> XdcodeResult<Self> {
        if !stack.ignore_user_and_project_exec_policy_rules()
            || !policy_files(stack)
                .await
                .map_err(|err| XdcodeErr::Fatal(format!("failed to load rules: {err}")))?
                .is_empty()
        {
            return Err(XdcodeErr::InvalidRequest(
                "inherited execution policy requires a managed-only destination".to_string(),
            ));
        }
        let destination = stack.requirements().exec_policy.as_deref();
        let compatible = match (snapshot.source_requirements.as_deref(), destination) {
            (None, None) => true,
            (Some(source), Some(target)) => {
                source == target
                    && source.as_ref().network_rules() == target.as_ref().network_rules()
                    && source.as_ref().host_executables() == target.as_ref().host_executables()
            }
            _ => false,
        };
        if !compatible {
            return Err(XdcodeErr::InvalidRequest(
                "inherited execution policy requirements differ from the destination".to_string(),
            ));
        }
        if let Some(requirements) = destination
            && !snapshot.policy.preserves_managed_requirements(requirements)
        {
            return Err(XdcodeErr::InvalidRequest(
                "inherited execution policy does not preserve destination requirements".to_string(),
            ));
        }
        Ok(Self::from_snapshot(
            Arc::clone(&snapshot.policy),
            snapshot.source_requirements.clone(),
        ))
    }
}
