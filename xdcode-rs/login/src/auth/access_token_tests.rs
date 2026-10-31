use super::*;

#[test]
fn classifies_personal_access_tokens_by_prefix() {
    assert!(matches!(
        classify_xdcode_access_token("at-example"),
        XdcodeAccessToken::PersonalAccessToken("at-example")
    ));
    assert!(matches!(
        classify_xdcode_access_token("header.payload.signature"),
        XdcodeAccessToken::AgentIdentityJwt("header.payload.signature")
    ));
}
