const PERSONAL_ACCESS_TOKEN_PREFIX: &str = "at-";

pub(super) enum XdcodeAccessToken<'a> {
    PersonalAccessToken(&'a str),
    AgentIdentityJwt(&'a str),
}

pub(super) fn classify_xdcode_access_token(access_token: &str) -> XdcodeAccessToken<'_> {
    if access_token.starts_with(PERSONAL_ACCESS_TOKEN_PREFIX) {
        XdcodeAccessToken::PersonalAccessToken(access_token)
    } else {
        XdcodeAccessToken::AgentIdentityJwt(access_token)
    }
}

#[cfg(test)]
#[path = "access_token_tests.rs"]
mod tests;
