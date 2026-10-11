//! Transparent Anthropic Messages reverse proxy to one configured upstream.
//!
//! Request and response bodies stream through unchanged as hyper bodies; nothing
//! is buffered, parsed or logged. The one exception is opt-in JSON minification,
//! which buffers and patches `POST /v1/messages` request bodies (see `minify`).
//! The upstream authority comes only from configuration, never from the request
//! target, `Host` header or credentials.

use std::{convert::Infallible, fmt, io, net::IpAddr, sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{Either, Full};
use hyper::{
    HeaderMap, Method, Request, Response, StatusCode, Uri, Version,
    body::Incoming,
    header::{self, HeaderName},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::{TokioExecutor, TokioIo, TokioTimer},
};
use tokio::{net::TcpListener, sync::Semaphore};

mod minify;

type HttpsClient = Client<hyper_rustls::HttpsConnector<HttpConnector>, minify::ReqBody>;
type ProxyBody = Either<Incoming, Full<Bytes>>;

/// The single trusted upstream. Plain `http` is accepted only for loopback hosts.
#[derive(Clone, Debug)]
pub struct Upstream {
    scheme: &'static str,
    authority: String,
    base_path: String,
    minify_json: bool,
}

impl Upstream {
    pub fn parse(url: &str) -> Result<Self, String> {
        let uri: Uri = url
            .parse()
            .map_err(|e| format!("invalid upstream URL: {e}"))?;
        let authority = uri.authority().ok_or("upstream URL needs a host")?;
        if authority.as_str().contains('@') {
            return Err("upstream URL must not contain credentials".into());
        }
        let scheme = match uri.scheme_str() {
            Some("https") => "https",
            Some("http") if is_loopback(authority.host()) => "http",
            Some("http") => return Err("plain http upstream is allowed only on loopback".into()),
            _ => return Err("upstream URL scheme must be https or http".into()),
        };
        if url.contains('#') {
            return Err("upstream URL must not have a fragment".into());
        }
        if authority.as_str().len() > authority.host().len()
            && matches!(authority.port_u16(), None | Some(0))
        {
            return Err("upstream URL port is invalid".into());
        }
        if uri.query().is_some() {
            return Err("upstream URL must not have a query".into());
        }
        Ok(Self {
            scheme,
            authority: authority.to_string(),
            base_path: uri.path().trim_end_matches('/').to_string(),
            minify_json: false,
        })
    }

    /// Enables minification of JSON in Bash tool results of `POST /v1/messages`.
    pub fn with_json_minification(mut self) -> Self {
        self.minify_json = true;
        self
    }

    /// Joins the configured base with the request's path and query. Only an
    /// origin-form path is accepted; any authority in the request is ignored.
    /// A path with a `..` segment is refused so it cannot leave the base path.
    fn target(&self, incoming: &Uri) -> Option<Uri> {
        let pq = incoming.path_and_query()?.as_str();
        if !pq.starts_with('/') || has_dot_dot_segment(incoming.path()) {
            return None;
        }
        format!("{}://{}{}{pq}", self.scheme, self.authority, self.base_path)
            .parse()
            .ok()
    }
}

/// Log-safe form: the base path is redacted because it may embed a credential.
impl fmt::Display for Upstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.scheme, self.authority)?;
        if !self.base_path.is_empty() {
            f.write_str("/[redacted]")?;
        }
        Ok(())
    }
}

/// True if the percent-decoded `path` has a `..` segment. Both `/` and `\` count
/// as separators because some upstreams treat `\` as one. Decoding is a single
/// pass; a malformed `%` escape is kept as literal bytes. A segment matches when
/// the part before its first `;` is `..`, because servlet-style upstreams strip
/// `;params` before resolving dot segments.
fn has_dot_dot_segment(path: &str) -> bool {
    let raw = path.as_bytes();
    let mut decoded = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let hex = raw
            .get(i + 1..i + 3)
            .filter(|h| raw[i] == b'%' && h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match hex {
            Some(b) => {
                decoded.push(b);
                i += 3;
            }
            None => {
                decoded.push(raw[i]);
                i += 1;
            }
        }
    }
    decoded
        .split(|&b| b == b'/' || b == b'\\')
        .any(|seg| seg.split(|&b| b == b';').next() == Some(b".."))
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Default cap on concurrently served client connections.
pub const MAX_CONNECTIONS: usize = 128;
/// Default time a client has to send a complete request head.
pub const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Default time allowed to open the TCP connection to the upstream.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Default time an unused pooled upstream connection stays open.
pub const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Default time a `--minify-json` request has to deliver its buffered body
/// once it holds an inspection permit. About 1 Mbit/s for a 32 MiB body.
pub const BODY_READ_TIMEOUT: Duration = Duration::from_secs(300);

/// Connection and timeout bounds. None of them limits how long a response
/// body may stream once the upstream has answered.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Further connections wait in the listen backlog until a slot frees.
    pub max_connections: usize,
    /// Also bounds how long an idle keep-alive client connection is held.
    pub header_read_timeout: Duration,
    pub connect_timeout: Duration,
    pub pool_idle_timeout: Duration,
    /// Bounds the buffered body read of a `--minify-json` request, so a stalled
    /// upload cannot hold an inspection permit forever. Other bodies stream.
    pub body_read_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            header_read_timeout: HEADER_READ_TIMEOUT,
            connect_timeout: CONNECT_TIMEOUT,
            pool_idle_timeout: POOL_IDLE_TIMEOUT,
            body_read_timeout: BODY_READ_TIMEOUT,
        }
    }
}

impl Limits {
    /// Zero would deadlock the accept loop or drop every connection; values
    /// beyond these bounds overflow tokio's semaphore or timer arithmetic.
    fn check(&self) -> io::Result<()> {
        const MAX_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
        let ok = (1..=Semaphore::MAX_PERMITS).contains(&self.max_connections)
            && [
                self.header_read_timeout,
                self.connect_timeout,
                self.pool_idle_timeout,
                self.body_read_timeout,
            ]
            .iter()
            .all(|d| !d.is_zero() && *d <= MAX_TIMEOUT);
        if ok {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("limits out of range: {self:?}"),
            ))
        }
    }
}

/// [`serve_with`] using [`Limits::default`].
pub async fn serve(listener: TcpListener, upstream: Upstream) -> io::Result<()> {
    serve_with(listener, upstream, Limits::default()).await
}

/// Accepts connections on `listener` and relays every request to `upstream`.
/// Fails before accepting anything unless `listener` is bound to a loopback
/// address: the proxy is unauthenticated and carries client credentials.
pub async fn serve_with(
    listener: TcpListener,
    upstream: Upstream,
    limits: Limits,
) -> io::Result<()> {
    limits.check()?;
    let addr = listener.local_addr()?;
    if !addr.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("listener must be a loopback address, got {addr}"),
        ));
    }
    // SSE is many small writes: disable Nagle on both legs.
    let mut http = HttpConnector::new();
    http.set_nodelay(true);
    http.enforce_http(false);
    http.set_connect_timeout(Some(limits.connect_timeout));
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_or_http()
        .enable_http1()
        .wrap_connector(http);
    let client: HttpsClient = Client::builder(TokioExecutor::new())
        .pool_idle_timeout(limits.pool_idle_timeout)
        .pool_timer(TokioTimer::new())
        .build(https);
    let slots = Arc::new(Semaphore::new(limits.max_connections));
    loop {
        // Waiting before accept leaves excess connections in the kernel backlog.
        let permit = slots
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore is never closed");
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            // Transient errors such as fd exhaustion must not end the proxy.
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let (client, upstream) = (client.clone(), upstream.clone());
        let svc = service_fn(move |req| {
            forward(
                client.clone(),
                upstream.clone(),
                limits.body_read_timeout,
                req,
            )
        });
        tokio::spawn(async move {
            // Client disconnects end this future, which drops the upstream body and connection.
            let _ = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(limits.header_read_timeout)
                .serve_connection(TokioIo::new(stream), svc)
                .await;
            drop(permit);
        });
    }
}

async fn forward(
    client: HttpsClient,
    upstream: Upstream,
    body_read_timeout: Duration,
    req: Request<Incoming>,
) -> Result<Response<ProxyBody>, Infallible> {
    let target = (req.method() != Method::CONNECT)
        .then(|| upstream.target(req.uri()))
        .flatten();
    let Some(target) = target else {
        return Ok(error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "unsupported request target",
        ));
    };
    let mut req = match minify::prepare(req, upstream.minify_json, body_read_timeout).await {
        Ok(req) => req,
        Err(status) => {
            return Ok(error(
                status,
                "invalid_request_error",
                "deslag: request body read failed",
            ));
        }
    };
    *req.uri_mut() = target;
    // Upstream always speaks HTTP/1.1, whatever version the client used.
    *req.version_mut() = Version::HTTP_11;
    strip_hop_by_hop(req.headers_mut());
    // The client derives Host from the configured upstream.
    req.headers_mut().remove(header::HOST);
    match client.request(req).await {
        Ok(resp) => {
            let (mut parts, body) = resp.into_parts();
            strip_hop_by_hop(&mut parts.headers);
            Ok(Response::from_parts(parts, Either::Left(body)))
        }
        // The error detail is dropped: it is not needed by the client and must never carry credentials.
        Err(_) => Ok(error(
            StatusCode::BAD_GATEWAY,
            "api_error",
            "deslag: upstream request failed",
        )),
    }
}

fn error(status: StatusCode, kind: &str, message: &str) -> Response<ProxyBody> {
    let body = format!(r#"{{"type":"error","error":{{"type":"{kind}","message":"{message}"}}}}"#);
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Either::Right(Full::new(Bytes::from(body))))
        .expect("static response parts are valid")
}

/// RFC 9110 §7.6.1 hop-by-hop headers, plus any named in `Connection`.
fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let named: Vec<HeaderName> = headers
        .get_all(header::CONNECTION)
        .iter()
        // Byte-wise so one obs-text segment cannot hide the valid names beside it.
        .flat_map(|v| v.as_bytes().split(|&b| b == b','))
        .filter_map(|n| HeaderName::from_bytes(n.trim_ascii()).ok())
        .collect();
    for name in named {
        headers.remove(name);
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-connection",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_validation() {
        for bad in [
            "http://example.com",
            "https://user:pw@example.com",
            "ftp://example.com",
            "https://example.com/?a=1",
            "https://example.com#frag",
            "https://example.com:99999",
            "https://example.com:0",
            "https://example.com:",
            "http://localhost.attacker.example",
            "http://127.0.0.1.attacker.example",
            "http://[::ffff:127.0.0.1]:9",
            "/relative",
            "not a url",
        ] {
            assert!(Upstream::parse(bad).is_err(), "{bad} accepted");
        }
        for good in [
            "https://api.anthropic.com",
            "https://example.com:443",
            "http://127.0.0.1:9",
            "http://[::1]:9",
            "http://localhost:9",
        ] {
            assert!(Upstream::parse(good).is_ok(), "{good} rejected");
        }
    }

    #[test]
    fn target_keeps_configured_authority_and_base_path() {
        let up = Upstream::parse("https://gw.example/anthropic/").unwrap();
        let t = |s: &str| up.target(&s.parse().unwrap()).map(|u| u.to_string());
        assert_eq!(
            t("/v1/messages?beta=true").as_deref(),
            Some("https://gw.example/anthropic/v1/messages?beta=true")
        );
        assert_eq!(
            t("http://evil.example/v1/models").as_deref(),
            Some("https://gw.example/anthropic/v1/models")
        );
        assert_eq!(t("*"), None);
    }

    #[test]
    fn dot_dot_segments_are_detected_in_any_encoding() {
        for bad in [
            "/..",
            "/../v1",
            "/v1/../../admin",
            "/v1/%2e%2e/x",
            "/v1/%2E%2e/x",
            "/v1/.%2e/x",
            "/v1/%2e./x",
            "/v1/..%2fx",
            "/v1/..%2Fx",
            "/v1/..\\x",
            "/v1/..%5cx",
            "/v1/..;/x",
            "/v1/%2e%2e;a/x",
        ] {
            assert!(has_dot_dot_segment(bad), "{bad} accepted");
        }
        for good in [
            "/",
            "/v1/messages",
            "/v1/./messages",
            "/v1/foo.bar",
            "/v1/..foo",
            "/v1/a..b",
            "/v1/foo..",
            "/v1/...",
            "/v1/%2e",
            "/v1/%zz",
            "/v1/%2",
            "/v1/.%+e",
            "/v1/a;..",
            "/v1/.;x",
        ] {
            assert!(!has_dot_dot_segment(good), "{good} rejected");
        }
    }

    #[test]
    fn target_rejects_dot_dot_path_but_not_query() {
        let up = Upstream::parse("https://gw.example/anthropic/").unwrap();
        let t = |s: &str| up.target(&s.parse().unwrap()).map(|u| u.to_string());
        assert_eq!(t("/../admin"), None);
        assert_eq!(t("/v1/%2e%2e/%2e%2e/admin"), None);
        assert_eq!(
            t("/v1/messages?next=../x").as_deref(),
            Some("https://gw.example/anthropic/v1/messages?next=../x")
        );
    }

    #[test]
    fn display_redacts_base_path() {
        let up = Upstream::parse("https://gw.example/token-secret/").unwrap();
        assert_eq!(up.to_string(), "https://gw.example/[redacted]");
        let up = Upstream::parse("https://api.anthropic.com").unwrap();
        assert_eq!(up.to_string(), "https://api.anthropic.com");
    }

    #[test]
    fn connection_options_survive_obs_text_segment() {
        let mut h = HeaderMap::new();
        h.insert(
            header::CONNECTION,
            header::HeaderValue::from_bytes(b"x-hop, \xff-opaque").unwrap(),
        );
        h.insert("x-hop", "1".parse().unwrap());
        h.insert("x-keep", "1".parse().unwrap());
        strip_hop_by_hop(&mut h);
        assert!(!h.contains_key("x-hop"));
        assert!(h.contains_key("x-keep"));
    }

    #[tokio::test]
    async fn serve_rejects_non_loopback_listener() {
        let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
        let up = Upstream::parse("https://api.anthropic.com").unwrap();
        let err = serve(listener, up).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[tokio::test]
    async fn serve_rejects_out_of_range_limits() {
        let d = Limits::default();
        for bad in [
            Limits {
                max_connections: 0,
                ..d
            },
            Limits {
                max_connections: Semaphore::MAX_PERMITS + 1,
                ..d
            },
            Limits {
                header_read_timeout: Duration::ZERO,
                ..d
            },
            Limits {
                connect_timeout: Duration::ZERO,
                ..d
            },
            Limits {
                pool_idle_timeout: Duration::MAX,
                ..d
            },
            Limits {
                body_read_timeout: Duration::ZERO,
                ..d
            },
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let up = Upstream::parse("https://api.anthropic.com").unwrap();
            let err = serve_with(listener, up, bad).await.unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{bad:?}");
        }
    }
}
