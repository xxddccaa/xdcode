//! Standalone consumer; prints event kinds without logging payloads.
use futures::StreamExt;
use xdcode_cloud_client::Client;
use xdcode_cloud_client::Credentials;
use xdcode_cloud_client::ResumeRequest;
use xdcode_http_client::HttpClientFactory;
use xdcode_http_client::OutboundProxyPolicy;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let endpoint = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected native gRPC origin"))?;
    let thread_id = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected existing thread ID"))?;
    let credentials = Credentials::new(
        &std::env::var("XDCODE_CLOUD_ACCESS_TOKEN")?,
        &std::env::var("XDCODE_CLOUD_ACCOUNT_ID")?,
    )?;
    let client = Client::new(
        &HttpClientFactory::new(OutboundProxyPolicy::RespectSystemProxy),
        &endpoint,
        credentials,
    )?;
    client
        .resume(&ResumeRequest {
            thread_id: thread_id.clone(),
            wait: false,
            shared_access: false,
        })
        .await?;
    let mut events = client.attach(&thread_id).await?;
    while let Some(event) = events.next().await {
        println!("{:?}", event?);
    }
    Ok(())
}
