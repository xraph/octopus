//! HTTP/3 (QUIC) listener for the Octopus API Gateway.
//!
//! HTTP/3 runs on a UDP socket *alongside* the TCP listener rather than
//! replacing it. Clients discover it through the `Alt-Svc` header advertised
//! on TCP responses (see `octopus_middleware::AltSvc`).
//!
//! # Request bodies are buffered
//!
//! An `h3` request body arrives as a stream of QUIC frames, not as an
//! `http_body::Body`. This listener collects it into `Full<Bytes>` before
//! dispatching. That matches what the TCP path already does — the gateway
//! buffers every request body before the middleware chain, and the SSE and
//! gRPC paths collect theirs too — so no streaming behaviour is lost relative
//! to HTTP/2. Response bodies *are* streamed frame by frame.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use http::{Request, Response};
use http_body::Body as HttpBody;
use http_body_util::Full;
use octopus_config::types::Http3Config;
use octopus_core::{Error, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{debug, error, warn};

/// Handles a request that arrived over HTTP/3.
///
/// Implemented by the runtime over its `RequestHandler`. Kept generic so this
/// crate does not depend on `octopus-runtime` (which would be circular).
#[async_trait::async_trait]
pub trait H3Handler: Send + Sync + 'static {
    /// Response body type, streamed back to the client frame by frame.
    type Body: HttpBody<Data = Bytes> + Send + Unpin + 'static;

    /// Handle one request.
    async fn handle(&self, req: Request<Full<Bytes>>) -> Result<Response<Self::Body>>;
}

/// A bound QUIC endpoint serving HTTP/3.
#[derive(Debug)]
pub struct Http3Listener {
    endpoint: quinn::Endpoint,
}

impl Http3Listener {
    /// Bind a UDP socket and prepare it to serve HTTP/3.
    ///
    /// `tls` must be a QUIC-compatible rustls config — TLS 1.3 only, with
    /// `h3` ALPN. Build it with
    /// [`octopus_tls::build_quic_server_config_from_pem`]; passing the TCP
    /// config (which permits TLS 1.2) is rejected here rather than producing
    /// a listener no client can negotiate with.
    pub fn bind(addr: SocketAddr, tls: rustls::ServerConfig, config: &Http3Config) -> Result<Self> {
        // quinn accepts a config carrying the TCP ALPN list without complaint —
        // it only cares that TLS 1.3 is available. The failure would surface
        // much later, as every client handshake failing to find a shared
        // protocol, so check it here where the error can name the cause.
        if !tls.alpn_protocols.iter().any(|p| p.as_slice() == b"h3") {
            return Err(Error::Config(format!(
                "QUIC TLS config does not offer the 'h3' ALPN protocol (offers: {}). \
                 Build it with octopus_tls::build_quic_server_config_from_pem.",
                tls.alpn_protocols
                    .iter()
                    .map(|p| String::from_utf8_lossy(p).into_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).map_err(|e| {
            Error::Config(format!(
                "TLS config is not usable for QUIC (QUIC mandates TLS 1.3): {e}"
            ))
        })?;

        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(crypto));

        let transport = Arc::get_mut(&mut server_config.transport)
            .ok_or_else(|| Error::Internal("QUIC transport config is shared".to_string()))?;
        transport.max_concurrent_bidi_streams(config.max_concurrent_bidi_streams.into());
        let idle: quinn::IdleTimeout = config.max_idle_timeout.try_into().map_err(|e| {
            Error::Config(format!(
                "http3.max_idle_timeout is out of range for QUIC: {e}"
            ))
        })?;
        transport.max_idle_timeout(Some(idle));

        let endpoint = quinn::Endpoint::server(server_config, addr)
            .map_err(|e| Error::Runtime(format!("Failed to bind QUIC socket on {addr}: {e}")))?;

        Ok(Self { endpoint })
    }

    /// The bound UDP address, including the OS-assigned port when 0 was given.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.endpoint
            .local_addr()
            .map_err(|e| Error::Runtime(format!("QUIC endpoint has no local address: {e}")))
    }

    /// Accept QUIC connections until the endpoint is closed, dispatching every
    /// request to `handler`.
    pub async fn serve<H: H3Handler>(self, handler: Arc<H>)
    where
        <H::Body as HttpBody>::Error: Send,
    {
        while let Some(incoming) = self.endpoint.accept().await {
            let handler = Arc::clone(&handler);
            tokio::spawn(async move {
                match incoming.await {
                    Ok(conn) => {
                        let remote = conn.remote_address();
                        if let Err(e) = serve_connection(conn, handler).await {
                            debug!(%remote, error = %e, "HTTP/3 connection ended");
                        }
                    }
                    Err(e) => debug!(error = %e, "QUIC handshake failed"),
                }
            });
        }
    }
}

/// Drive one QUIC connection: HTTP/3 handshake, then a request per stream.
async fn serve_connection<H: H3Handler>(conn: quinn::Connection, handler: Arc<H>) -> Result<()>
where
    <H::Body as HttpBody>::Error: Send,
{
    let mut h3_conn = h3::server::Connection::new(h3_quinn::Connection::new(conn))
        .await
        .map_err(|e| Error::Runtime(format!("HTTP/3 handshake failed: {e}")))?;

    loop {
        match h3_conn.accept().await {
            Ok(Some(resolver)) => {
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    // h3 hands back a resolver so the caller decides when to
                    // pay for header decoding; resolving yields the request
                    // and its bidirectional stream.
                    match resolver.resolve_request().await {
                        Ok((req, stream)) => {
                            if let Err(e) = serve_request(req, stream, handler).await {
                                warn!(error = %e, "HTTP/3 request failed");
                            }
                        }
                        Err(e) => warn!(error = %e, "HTTP/3 request header decode failed"),
                    }
                });
            }
            Ok(None) => return Ok(()),
            Err(e) => {
                return Err(Error::Runtime(format!("HTTP/3 stream error: {e}")));
            }
        }
    }
}

/// Read one request off its QUIC stream, dispatch it, stream the response back.
async fn serve_request<H, S>(
    req: Request<()>,
    mut stream: h3::server::RequestStream<S, Bytes>,
    handler: Arc<H>,
) -> Result<()>
where
    H: H3Handler,
    <H::Body as HttpBody>::Error: Send,
    S: h3::quic::BidiStream<Bytes>,
{
    // Collect the request body. See the module docs: the gateway buffers every
    // request body on the TCP path too, so this loses nothing relative to h2.
    let mut body = BytesMut::new();
    loop {
        match stream.recv_data().await {
            Ok(Some(mut chunk)) => {
                while chunk.has_remaining() {
                    let bytes = chunk.chunk().to_vec();
                    body.put_slice(&bytes);
                    let n = bytes.len();
                    chunk.advance(n);
                }
            }
            Ok(None) => break,
            Err(e) => {
                return Err(Error::InvalidRequest(format!(
                    "HTTP/3 body read failed: {e}"
                )))
            }
        }
    }

    let (mut parts, ()) = req.into_parts();
    // Mark the transport so downstream code (Alt-Svc suppression, logging,
    // metrics) can tell an h3 request from an h2 one.
    parts.version = http::Version::HTTP_3;
    let req = Request::from_parts(parts, Full::new(body.freeze()));

    let resp = match handler.handle(req).await {
        Ok(resp) => resp,
        Err(e) => {
            error!(error = %e, "HTTP/3 handler error");
            let resp = Response::builder()
                .status(e.to_status_code())
                .body(())
                .map_err(|e| Error::Internal(format!("failed to build error response: {e}")))?;
            stream
                .send_response(resp)
                .await
                .map_err(|e| Error::Runtime(format!("failed to send HTTP/3 response: {e}")))?;
            return stream
                .finish()
                .await
                .map_err(|e| Error::Runtime(format!("failed to finish HTTP/3 stream: {e}")));
        }
    };

    let (parts, body) = resp.into_parts();
    stream
        .send_response(Response::from_parts(parts, ()))
        .await
        .map_err(|e| Error::Runtime(format!("failed to send HTTP/3 response: {e}")))?;

    let mut body = std::pin::pin!(body);
    while let Some(frame) = std::future::poll_fn(|cx| body.as_mut().poll_frame(cx)).await {
        let frame =
            frame.map_err(|_| Error::Internal("response body stream failed".to_string()))?;
        if let Ok(data) = frame.into_data() {
            stream
                .send_data(data)
                .await
                .map_err(|e| Error::Runtime(format!("failed to send HTTP/3 body: {e}")))?;
        }
    }

    stream
        .finish()
        .await
        .map_err(|e| Error::Runtime(format!("failed to finish HTTP/3 stream: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_config::types::Http3Config;
    use std::net::SocketAddr;

    const TEST_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBqTCCAVCgAwIBAgIUdXZJNtio8+gPkOsw2TsTczF8LiAwCgYIKoZIzj0EAwIw
GDEWMBQGA1UEAwwNb2N0b3B1cy5sb2NhbDAeFw0yNjA1MzAxOTAxMDBaFw0zNjA1
MjcxOTAxMDBaMBgxFjAUBgNVBAMMDW9jdG9wdXMubG9jYWwwWTATBgcqhkjOPQIB
BggqhkjOPQMBBwNCAARg9r23sThOLJ0CVVqTeLLbkQSbl/fAMZJwLhzCrGHJXk0e
xP7K73agVp3RiDz7w/rmMBCmhSCppD+vpl7vMnZ9o3gwdjAdBgNVHQ4EFgQU4Lgf
Lbz635DVurCsZ3dWSqQ2eJAwHwYDVR0jBBgwFoAU4LgfLbz635DVurCsZ3dWSqQ2
eJAwDwYDVR0TAQH/BAUwAwEB/zAjBgNVHREEHDAagg1vY3RvcHVzLmxvY2Fsggls
b2NhbGhvc3QwCgYIKoZIzj0EAwIDRwAwRAIgZo1rDiv07r7Sc8bMkOb/WVCmL6m8
AbWTroKXTQjea7oCIFC3gsegwlyDazwLWcXPoq/9orb8RokhQlRjTtmCzW6P
-----END CERTIFICATE-----
";

    const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgdWBkKWLdsDaJ1ERt
VsIFX7+uAgAU2d0mbk+Hls1GCeKhRANCAARg9r23sThOLJ0CVVqTeLLbkQSbl/fA
MZJwLhzCrGHJXk0exP7K73agVp3RiDz7w/rmMBCmhSCppD+vpl7vMnZ9
-----END PRIVATE KEY-----
";

    fn any_port() -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }

    /// Binding must actually open a UDP socket and report the port the OS
    /// assigned, so the caller can advertise it in `Alt-Svc`.
    #[tokio::test]
    async fn bind_opens_a_udp_socket_and_reports_its_port() {
        let tls = octopus_tls::build_quic_server_config_from_pem(
            TEST_CERT.as_bytes(),
            TEST_KEY.as_bytes(),
        )
        .expect("test cert should build a QUIC config");

        let listener = Http3Listener::bind(any_port(), tls, &Http3Config::default())
            .expect("binding an ephemeral UDP port should succeed");

        assert_ne!(
            listener.local_addr().unwrap().port(),
            0,
            "the OS-assigned port must be reported so Alt-Svc can advertise it"
        );
    }

    /// The TCP TLS config offers `h2`/`http/1.1` ALPN. quinn itself accepts
    /// such a config (it simply negotiates TLS 1.3), so nothing would fail
    /// until a client tried to connect and found no shared protocol. Reject it
    /// at bind time instead of serving a listener no HTTP/3 client can talk to.
    #[tokio::test]
    async fn bind_rejects_the_tcp_tls_config() {
        let tcp_tls =
            octopus_tls::build_server_config_from_pem(TEST_CERT.as_bytes(), TEST_KEY.as_bytes())
                .expect("test cert should build a TCP config");

        let err = Http3Listener::bind(any_port(), tcp_tls, &Http3Config::default())
            .expect_err("a TLS 1.2-capable config is not usable for QUIC");

        assert!(
            err.to_string().to_lowercase().contains("quic"),
            "error should explain this is a QUIC constraint, got: {err}"
        );
    }
}
