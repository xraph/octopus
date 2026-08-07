//! `Alt-Svc` advertisement for the HTTP/3 listener.
//!
//! HTTP/3 is not negotiated in-band: a client arriving over TCP has no way to
//! know the origin also speaks QUIC. RFC 9114 §3.1.1 has the server advertise
//! it with an `Alt-Svc` header, which the client caches and then races a QUIC
//! connection against its existing one on a later request.
//!
//! Without this header the UDP listener is effectively invisible to browsers,
//! which is why it is on by default whenever HTTP/3 is enabled.

use async_trait::async_trait;
use http::{HeaderName, HeaderValue, Request, Response, Version};
use octopus_core::{Body, Middleware, Next, Result};

const ALT_SVC: HeaderName = HeaderName::from_static("alt-svc");

/// Adds `Alt-Svc` to responses served over TCP, advertising the HTTP/3 listener.
#[derive(Debug, Clone)]
pub struct AltSvc {
    value: HeaderValue,
}

impl AltSvc {
    /// Build from a pre-rendered header value, e.g. `h3=":443"; ma=86400`.
    ///
    /// Returns `None` when the value is not a valid header value, so a
    /// malformed port or max-age cannot inject a second header line.
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        HeaderValue::from_str(value).ok().map(|value| Self { value })
    }
}

#[async_trait]
impl Middleware for AltSvc {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // A request that already arrived over HTTP/3 needs no advertisement —
        // the client is plainly already using it.
        let already_h3 = req.version() == Version::HTTP_3;

        let mut resp = next.run(req).await?;

        if !already_h3 {
            resp.headers_mut().insert(ALT_SVC, self.value.clone());
        }

        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::StatusCode;
    use http_body_util::Full;
    use octopus_core::middleware::HandlerFn;
    use std::sync::Arc;

    fn next() -> Next {
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::from(Vec::new());
        let final_handler: HandlerFn = Box::new(|_req| {
            Box::pin(async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(Full::new(Bytes::from_static(b"ok")))
                    .unwrap())
            })
        });
        Next::with_handler(stack, final_handler)
    }

    fn request(version: Version) -> Request<Body> {
        Request::builder()
            .method(http::Method::GET)
            .uri("/")
            .version(version)
            .body(Full::new(Bytes::new()))
            .unwrap()
    }

    #[tokio::test]
    async fn advertises_h3_on_a_tcp_response() {
        let mw = AltSvc::new("h3=\":8443\"; ma=86400").unwrap();

        let resp = mw.call(request(Version::HTTP_2), next()).await.unwrap();

        assert_eq!(
            resp.headers().get("alt-svc").unwrap(),
            "h3=\":8443\"; ma=86400"
        );
    }

    #[tokio::test]
    async fn does_not_advertise_to_a_client_already_on_h3() {
        let mw = AltSvc::new("h3=\":8443\"; ma=86400").unwrap();

        let resp = mw.call(request(Version::HTTP_3), next()).await.unwrap();

        assert!(
            resp.headers().get("alt-svc").is_none(),
            "a client already speaking h3 does not need the advertisement"
        );
    }

    #[tokio::test]
    async fn advertises_on_http1_as_well() {
        let mw = AltSvc::new("h3=\":443\"; ma=3600").unwrap();

        let resp = mw.call(request(Version::HTTP_11), next()).await.unwrap();

        assert_eq!(resp.headers().get("alt-svc").unwrap(), "h3=\":443\"; ma=3600");
    }

    #[test]
    fn rejects_a_value_that_is_not_a_valid_header() {
        assert!(AltSvc::new("h3=\":443\"\n injected: yes").is_none());
    }
}
