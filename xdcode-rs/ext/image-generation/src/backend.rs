use http::HeaderMap;
use http::HeaderValue;
use xdcode_api::ImageEditRequest;
use xdcode_api::ImageGenerationRequest;
use xdcode_api::ImageRequestError;
use xdcode_api::ImageResponse;
use xdcode_api::ImagesClient;
use xdcode_api::ReqwestTransport;
use xdcode_api::map_api_error;
use xdcode_core::X_XDCODE_TURN_METADATA_HEADER;
use xdcode_http_client::ClientRouteClass;
use xdcode_http_client::HttpClientFactory;
use xdcode_login::default_client::add_originator_header;
use xdcode_login::default_client::create_transport_for_routes_async;
use xdcode_model_provider::SharedModelProvider;
use xdcode_protocol::error::XdcodeErr;

const X_XDCODE_IMAGE_TURN_ID_HEADER: &str = "x-xdcode-image-turn-id";

pub(crate) struct ImageBackendError {
    message: String,
    xdcode_error: XdcodeErr,
    imagegen_request_id: Option<String>,
}

impl ImageBackendError {
    fn from_image_request(error: ImageRequestError) -> Self {
        let (error, imagegen_request_id) = error.into_parts();
        let message = error.to_string();
        Self {
            message,
            xdcode_error: map_api_error(error),
            imagegen_request_id,
        }
    }

    fn from_message(message: String) -> Self {
        Self {
            xdcode_error: XdcodeErr::Stream(message.clone()),
            message,
            imagegen_request_id: None,
        }
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn xdcode_error(&self) -> &XdcodeErr {
        &self.xdcode_error
    }

    pub(crate) fn imagegen_request_id(&self) -> Option<&str> {
        self.imagegen_request_id.as_deref()
    }
}

#[derive(Clone)]
pub(crate) struct XdcodeImagesBackend {
    provider: SharedModelProvider,
    http_client_factory: HttpClientFactory,
    originator: Option<String>,
}

impl XdcodeImagesBackend {
    /// Creates a backend that sends image requests through the active model provider.
    pub(crate) fn new(
        provider: SharedModelProvider,
        http_client_factory: HttpClientFactory,
        originator: Option<String>,
    ) -> Self {
        Self {
            provider,
            http_client_factory,
            originator,
        }
    }

    /// Resolves the provider and auth required for the current image API request.
    async fn client(&self) -> Result<ImagesClient<ReqwestTransport>, ImageBackendError> {
        let provider = self
            .provider
            .api_provider()
            .await
            .map_err(|err| ImageBackendError::from_message(err.to_string()))?;
        let auth = self
            .provider
            .api_auth()
            .await
            .map_err(|err| ImageBackendError::from_message(err.to_string()))?;
        let transport = create_transport_for_routes_async(
            self.http_client_factory.clone(),
            ClientRouteClass::Api,
        )
        .await
        .map_err(|err| ImageBackendError::from_message(err.to_string()))?;
        Ok(ImagesClient::new(transport, provider, auth))
    }

    /// Sends a standalone image generation request through the configured Images client.
    pub(crate) async fn generate(
        &self,
        request: ImageGenerationRequest,
        turn_id: &str,
        xdcode_turn_metadata: Option<&str>,
    ) -> Result<(ImageResponse, Option<String>), ImageBackendError> {
        self.client()
            .await?
            .generate(
                &request,
                image_request_headers(self.originator.as_deref(), turn_id, xdcode_turn_metadata),
            )
            .await
            .map_err(ImageBackendError::from_image_request)
    }

    /// Sends a standalone image edit request through the configured Images client.
    pub(crate) async fn edit(
        &self,
        request: ImageEditRequest,
        turn_id: &str,
        xdcode_turn_metadata: Option<&str>,
    ) -> Result<(ImageResponse, Option<String>), ImageBackendError> {
        self.client()
            .await?
            .edit(
                &request,
                image_request_headers(self.originator.as_deref(), turn_id, xdcode_turn_metadata),
            )
            .await
            .map_err(ImageBackendError::from_image_request)
    }
}

fn image_request_headers(
    originator: Option<&str>,
    turn_id: &str,
    xdcode_turn_metadata: Option<&str>,
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(turn_id) = HeaderValue::from_str(turn_id) {
        headers.insert(X_XDCODE_IMAGE_TURN_ID_HEADER, turn_id);
    }
    if let Some(originator) = originator {
        add_originator_header(&mut headers, originator);
    }

    if let Some(metadata) = xdcode_turn_metadata
        && let Ok(metadata) = HeaderValue::from_str(metadata)
    {
        headers.insert(X_XDCODE_TURN_METADATA_HEADER, metadata);
    }

    headers
}
