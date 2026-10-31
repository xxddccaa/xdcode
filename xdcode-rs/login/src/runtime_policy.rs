//! Hosted account APIs remain type-compatible but are unavailable in xdcode.

pub(crate) fn reject_hosted_auth() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        xdcode_model_provider_info::CUSTOM_PROVIDER_SETUP,
    ))
}
