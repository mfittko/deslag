//! Transparent Anthropic Messages reverse proxy to one configured upstream.
//!
//! Request and response bodies stream through unchanged as hyper bodies; nothing
//! is buffered, parsed or logged. The upstream authority comes only from
//! configuration, never from the request target, `Host` header or credentials.

use std::{convert::Infallible, fmt, net::IpAddr, time::Duration};

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
    rt::{TokioExecutor, TokioIo},
};
use tokio::net::TcpListener;

type HttpsClient = Client<hyper_rustls::HttpsConnector<HttpConnector>, Incoming>;
type ProxyBody = Either<Incoming, Full<Bytes>>;

/// The single trusted upstream. Plain `http` is accepted only for loopback hosts.
#[derive(Clone, Debug)]
pub struct Upstream {
    scheme: &'static str,
    authority: String,
    base_path: String,
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
        if authority.as_str().len() > authority.host().len() && authority.port_u16().is_none() {
            return Err("upstream URL port is invalid".into());
        }
        if uri.query().is_some() {
            return Err("upstream URL must not have a query".into());
        }
        Ok(Self {
            scheme,
            authority: authority.to_string(),
            base_path: uri.path().trim_end_matches('/').to_string(),
        })
    }

    /// Joins the configured base with the request's path and query. Only an
    /// origin-form path is accepted; any authority in the request is ignored.
    fn target(&self, incoming: &Uri) -> Option<Uri> {
        let pq = incoming.path_and_query()?.as_str();
        if !pq.starts_with('/') {
            return None;
        }
        format!("{}://{}{}{pq}", self.scheme, self.authority, self.base_path)
            .parse()
            .ok()
    }
}

impl fmt::Display for Upstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}{}", self.scheme, self.authority, self.base_path)
    }
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Accepts connections on `listener` and relays every request to `upstream`.
pub async fn serve(listener: TcpListener, upstream: Upstream) {
    // SSE is many small writes: disable Nagle on both legs.
    let mut http = HttpConnector::new();
    http.set_nodelay(true);
    http.enforce_http(false);
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_or_http()
        .enable_http1()
        .wrap_connector(http);
    let client: HttpsClient = Client::builder(TokioExecutor::new()).build(https);
    loop {
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
        let svc = service_fn(move |req| forward(client.clone(), upstream.clone(), req));
        tokio::spawn(async move {
            // Client disconnects end this future, which drops the upstream body and connection.
            let _ = http1::Builder::new()
                .serve_connection(TokioIo::new(stream), svc)
                .await;
        });
    }
}

async fn forward(
    client: HttpsClient,
    upstream: Upstream,
    mut req: Request<Incoming>,
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
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|n| HeaderName::from_bytes(n.trim().as_bytes()).ok())
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
            "/relative",
            "not a url",
        ] {
            assert!(Upstream::parse(bad).is_err(), "{bad} accepted");
        }
        for good in [
            "https://api.anthropic.com",
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
}
