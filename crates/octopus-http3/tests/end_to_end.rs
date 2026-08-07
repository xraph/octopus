//! End-to-end proof that the listener actually serves HTTP/3 over QUIC.
//!
//! Binding a UDP socket is not the same as speaking HTTP/3. This drives a real
//! `h3` client against the listener: QUIC handshake, request, response headers,
//! and a streamed response body.

use bytes::{Buf, Bytes};
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use octopus_config::types::Http3Config;
use octopus_core::Result;
use octopus_http3::{H3Handler, Http3Listener};
use std::net::SocketAddr;
use std::sync::Arc;

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

/// Echoes the request path back, so the test proves the request survived the
/// QUIC/HTTP3 round trip rather than merely that something responded.
struct EchoPathHandler;

#[async_trait::async_trait]
impl H3Handler for EchoPathHandler {
    type Body = Full<Bytes>;

    async fn handle(&self, req: Request<Full<Bytes>>) -> Result<Response<Self::Body>> {
        let version = req.version();
        let body = format!("{} {:?}", req.uri().path(), version);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("x-served-by", "octopus-http3")
            .body(Full::new(Bytes::from(body)))
            .unwrap())
    }
}

/// Test-only certificate verifier that accepts the fixture certificate.
///
/// The shared test cert is self-signed with `CA:TRUE`, which rustls refuses to
/// accept as an end-entity certificate. Chain validation is not what this test
/// is exercising — the QUIC and HTTP/3 layers are — so verification is skipped
/// here the same way quinn's own examples do it.
#[derive(Debug)]
struct AcceptFixtureCert(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptFixtureCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn client_endpoint() -> quinn::Endpoint {
    let provider = Arc::new(rustls::crypto::ring::default_provider());

    let mut crypto = rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptFixtureCert(provider)))
        .with_no_client_auth();
    crypto.alpn_protocols = vec![b"h3".to_vec()];

    let client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
    ));

    let mut endpoint =
        quinn::Endpoint::client("127.0.0.1:0".parse::<SocketAddr>().unwrap()).unwrap();
    endpoint.set_default_client_config(client_config);
    endpoint
}

#[tokio::test]
async fn serves_a_request_over_quic() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let tls = octopus_tls::build_quic_server_config_from_pem(
        TEST_CERT.as_bytes(),
        TEST_KEY.as_bytes(),
    )
    .unwrap();

    let listener = Http3Listener::bind(
        "127.0.0.1:0".parse().unwrap(),
        tls,
        &Http3Config::default(),
    )
    .unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(listener.serve(Arc::new(EchoPathHandler)));

    // --- client side ---
    let endpoint = client_endpoint();
    let conn = endpoint
        .connect(addr, "localhost")
        .unwrap()
        .await
        .expect("QUIC handshake with the listener should succeed");

    let (mut driver, mut send_request) = h3::client::new(h3_quinn::Connection::new(conn))
        .await
        .expect("HTTP/3 handshake should succeed");

    let drive = tokio::spawn(async move { std::future::poll_fn(|cx| driver.poll_close(cx)).await });

    let req = Request::builder()
        .method("GET")
        .uri("https://localhost/hello")
        .body(())
        .unwrap();

    let mut stream = send_request.send_request(req).await.unwrap();
    stream.finish().await.unwrap();

    let resp = stream.recv_response().await.expect("a response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("x-served-by").unwrap(), "octopus-http3");

    let mut body = Vec::new();
    while let Some(mut chunk) = stream.recv_data().await.unwrap() {
        while chunk.has_remaining() {
            let n = {
                let c = chunk.chunk();
                body.extend_from_slice(c);
                c.len()
            };
            chunk.advance(n);
        }
    }

    assert_eq!(
        String::from_utf8(body).unwrap(),
        "/hello HTTP/3.0",
        "the path must survive the round trip and the handler must see HTTP/3"
    );

    drop(send_request);
    let _ = drive.await;
}
