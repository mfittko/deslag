//! Proxy-mode replay and compression ownership through the real proxy and a
//! local mock upstream. Each proxy instance is one process lifetime; a new one
//! on the same state directory is a restart or a resume.

use std::{
    convert::Infallible,
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use bytes::Bytes;
use deslag_proxy::{
    Upstream, serve,
    session::{Mode, Owner, Store, manifest_path, write_owner},
};
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

const SCOPE: &str = "session-a";

/// Pretty JSON that json-minify shortens, and plain text it leaves alone.
const R1: &str = "{\n  \"rows\": [\n    1,\n    2\n  ]\n}\n";
const R2: &str = "plain  text";
const R3: &str = "{ \"ok\" : true }";
const R4: &str = "[ 1, 2, 3 ]";

fn min(s: &str) -> String {
    serde_json::to_string(&serde_json::from_str::<Value>(s).unwrap()).unwrap()
}

fn temp_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "deslag-replay-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// One proxy process lifetime with its own mock upstream.
struct Proxy {
    addr: SocketAddr,
    seen: mpsc::UnboundedReceiver<Bytes>,
}

impl Proxy {
    async fn start(dir: &Path, mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let up = listener.local_addr().unwrap();
        let (tx, seen) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let tx = tx.clone();
                let svc = service_fn(move |req: Request<Incoming>| {
                    let tx = tx.clone();
                    async move {
                        let _ = tx.send(req.into_body().collect().await.unwrap().to_bytes());
                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"{}"))))
                    }
                });
                tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(stream), svc));
            }
        });
        let store = Store {
            dir: dir.to_path_buf(),
            mode,
        };
        let upstream = Upstream::parse(&format!("http://{up}"))
            .unwrap()
            .with_json_minification(store);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve(listener, upstream));
        Self { addr, seen }
    }

    /// Sends `body`; returns the upstream body, or the status and error body
    /// when the proxy rejected the request without calling the upstream.
    async fn send(&mut self, scope: Option<&str>, body: Vec<u8>) -> Result<Bytes, (u16, Value)> {
        let stream = TcpStream::connect(self.addr).await.unwrap();
        let (mut tx, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .unwrap();
        tokio::spawn(conn);
        let mut req = Request::post("/v1/messages");
        if let Some(scope) = scope {
            req = req.header("x-claude-code-session-id", scope);
        }
        let resp = tx
            .send_request(req.body(Full::new(Bytes::from(body))).unwrap())
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let reply = resp.into_body().collect().await.unwrap().to_bytes();
        if status == 200 {
            return Ok(self.seen.recv().await.unwrap());
        }
        // The upstream records a body before it answers, so this is final.
        assert!(
            self.seen.try_recv().is_err(),
            "rejected request reached upstream"
        );
        Err((status, serde_json::from_slice(&reply).unwrap()))
    }

    async fn turn(&mut self, model: &str, results: &[(&str, &str)]) -> Result<Bytes, (u16, Value)> {
        self.send(Some(SCOPE), conversation(model, results)).await
    }
}

/// A Messages request: one prompt, then a Bash tool_use/tool_result pair per result.
fn conversation(model: &str, results: &[(&str, &str)]) -> Vec<u8> {
    let mut messages = vec![json!({"role": "user", "content": "inspect the data"})];
    for (id, content) in results {
        messages.push(json!({"role": "assistant", "content": [
            {"type": "tool_use", "id": id, "name": "Bash", "input": {"command": "cat x"}}
        ]}));
        messages.push(json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": id, "content": content}
        ]}));
    }
    serde_json::to_vec(&json!({"max_tokens": 64, "messages": messages, "model": model})).unwrap()
}

/// The later upstream body starts with every message byte of the earlier one.
fn assert_prefix_stable(earlier: &Bytes, later: &Bytes) {
    let end = earlier.windows(2).rposition(|w| w == b"],").unwrap();
    assert!(
        later.starts_with(&earlier[..end]),
        "prefix drift:\n{}\n{}",
        String::from_utf8_lossy(earlier),
        String::from_utf8_lossy(later)
    );
}

fn content_of(body: &Bytes, id: &str) -> String {
    let req: Value = serde_json::from_slice(body).unwrap();
    req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .find(|b| b["tool_use_id"] == id)
        .and_then(|b| b["content"].as_str())
        .unwrap()
        .to_owned()
}

fn manifest(dir: &Path) -> Value {
    serde_json::from_slice(&fs::read(manifest_path(dir, SCOPE)).unwrap()).unwrap()
}

fn assert_rejected(got: Result<Bytes, (u16, Value)>, case: &str) {
    let (status, body) = got.expect_err(case);
    assert_eq!(status, 400, "{case}");
    assert_eq!(body["error"]["type"], "invalid_request_error", "{case}");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("start a new session"), "{case}: {message}");
}

#[tokio::test]
async fn resume_and_restart_keep_history_byte_stable() {
    let dir = temp_dir();
    write_owner(&dir, Owner::Proxy).unwrap();
    let mut p = Proxy::start(&dir, Mode::Safe).await;
    let t0 = p.turn("m", &[]).await.unwrap();
    let t1 = p.turn("m", &[("toolu_1", R1)]).await.unwrap();
    let t2 = p
        .turn("m", &[("toolu_1", R1), ("toolu_2", R2)])
        .await
        .unwrap();
    let resent = p
        .turn("m", &[("toolu_1", R1), ("toolu_2", R2)])
        .await
        .unwrap();
    assert_eq!(resent, t2, "identical resend is byte-identical");
    drop(p);

    // Restart or resume: a new proxy process; only the manifest on disk survives.
    let mut p = Proxy::start(&dir, Mode::Safe).await;
    let t3 = p
        .turn("m", &[("toolu_1", R1), ("toolu_2", R2), ("toolu_3", R3)])
        .await
        .unwrap();
    for (a, b) in [(&t0, &t1), (&t1, &t2), (&t2, &t3)] {
        assert_prefix_stable(a, b);
    }
    assert_eq!(content_of(&t3, "toolu_1"), min(R1));
    assert_eq!(content_of(&t3, "toolu_2"), R2);
    assert_eq!(content_of(&t3, "toolu_3"), min(R3));
    let m = manifest(&dir);
    assert_eq!(
        (m["version"].clone(), m["owner"].clone()),
        (json!(1), json!("proxy"))
    );
    assert_eq!(m["mode"], "safe");
    assert_eq!(m["decisions"]["toolu_2"]["unchanged"], true);
    let encoded = &m["decisions"]["toolu_1"]["encoded"];
    assert_eq!(
        *encoded,
        json!({"transform": deslag_core::JSON_MINIFY_ID, "version": deslag_core::JSON_MINIFY_VERSION})
    );
    let raw = fs::read_to_string(manifest_path(&dir, SCOPE)).unwrap();
    assert!(
        !raw.contains("rows") && !raw.contains(SCOPE),
        "no tool body or raw scope stored"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn disable_keeps_pinned_decisions_and_records_new_results_unchanged() {
    let dir = temp_dir();
    write_owner(&dir, Owner::Proxy).unwrap();
    let mut p = Proxy::start(&dir, Mode::Safe).await;
    p.turn("m", &[]).await.unwrap();
    let t1 = p.turn("m", &[("toolu_1", R1)]).await.unwrap();
    for disabled in [Owner::None, Owner::Hook] {
        write_owner(&dir, disabled).unwrap();
        let t2 = p
            .turn("m", &[("toolu_1", R1), ("toolu_3", R3)])
            .await
            .unwrap();
        assert_prefix_stable(&t1, &t2);
        assert_eq!(content_of(&t2, "toolu_1"), min(R1), "{disabled:?}");
        assert_eq!(content_of(&t2, "toolu_3"), R3, "{disabled:?}");
    }
    assert_eq!(manifest(&dir)["decisions"]["toolu_3"]["unchanged"], true);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn skipped_result_stays_unchanged_after_reenable() {
    let dir = temp_dir();
    write_owner(&dir, Owner::Proxy).unwrap();
    let mut p = Proxy::start(&dir, Mode::Safe).await;
    p.turn("m", &[]).await.unwrap();
    write_owner(&dir, Owner::None).unwrap();
    let skipped = p.turn("m", &[("toolu_1", R1)]).await.unwrap();
    assert_eq!(content_of(&skipped, "toolu_1"), R1);
    write_owner(&dir, Owner::Proxy).unwrap();
    let later = p
        .turn("m", &[("toolu_1", R1), ("toolu_3", R3)])
        .await
        .unwrap();
    assert_prefix_stable(&skipped, &later);
    assert_eq!(
        content_of(&later, "toolu_1"),
        R1,
        "first decision was unchanged"
    );
    assert_eq!(content_of(&later, "toolu_3"), min(R3));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn model_switch_records_model_per_decision() {
    let dir = temp_dir();
    write_owner(&dir, Owner::Proxy).unwrap();
    let mut p = Proxy::start(&dir, Mode::Safe).await;
    p.turn("m", &[]).await.unwrap();
    let before = p.turn("m", &[("toolu_1", R1)]).await.unwrap();
    let after = p
        .turn("m2", &[("toolu_1", R1), ("toolu_3", R3)])
        .await
        .unwrap();
    assert_prefix_stable(&before, &after);
    assert_eq!(content_of(&after, "toolu_1"), min(R1));
    let m = manifest(&dir);
    assert_eq!(m["model_profile"], "m");
    assert_eq!(m["decisions"]["toolu_1"]["model_profile"], "m");
    assert_eq!(m["decisions"]["toolu_3"]["model_profile"], "m2");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn missing_or_corrupt_state_fails_explicitly() {
    let dir = temp_dir();
    write_owner(&dir, Owner::Proxy).unwrap();
    let mut p = Proxy::start(&dir, Mode::Safe).await;
    let history = [("toolu_1", R1), ("toolu_2", R2)];
    // A proxy added to a session with existing history.
    assert_rejected(p.turn("m", &history).await, "no manifest");
    assert!(!manifest_path(&dir, SCOPE).exists());

    p.turn("m", &[]).await.unwrap();
    p.turn("m", &[("toolu_1", R1)]).await.unwrap();
    let good = manifest(&dir);
    let path = manifest_path(&dir, SCOPE);
    let with = |pointer: &str, value: Value| {
        let mut m = good.clone();
        *m.pointer_mut(pointer).unwrap() = value;
        m.to_string().into_bytes()
    };
    let without = |key: &str| {
        let mut m = good.clone();
        m.as_object_mut().unwrap().remove(key).unwrap();
        m.to_string().into_bytes()
    };
    for (case, bytes) in [
        ("unparseable", b"{ not json".to_vec()),
        ("no decisions", br#"{"version":1}"#.to_vec()),
        ("array", b"[]".to_vec()),
        ("unsupported version", with("/version", json!(2))),
        (
            "retired transform",
            with("/decisions/toolu_1/encoded/version", json!(2)),
        ),
        (
            "corrupt decision",
            with("/decisions/toolu_1/encoded/version", json!("1")),
        ),
        (
            "digest mismatch",
            with("/decisions/toolu_1/digest", json!("00")),
        ),
        ("owner missing", without("owner")),
        ("mode missing", without("mode")),
        ("owner not a string", with("/owner", json!(1))),
        ("owner unknown", with("/owner", json!("hook"))),
        ("mode not a string", with("/mode", json!(true))),
        ("mode unknown", with("/mode", json!("aggressive"))),
    ] {
        fs::write(&path, &bytes).unwrap();
        assert_rejected(p.turn("m", &history).await, case);
        assert_eq!(fs::read(&path).unwrap(), bytes, "{case}: never overwritten");
    }
    fs::write(&path, good.to_string()).unwrap();
    let undecided = [("toolu_1", R1), ("toolu_9", R3), ("toolu_2", R2)];
    assert_rejected(p.turn("m", &undecided).await, "undecided history");
    fs::remove_file(&path).unwrap();
    assert_rejected(p.turn("m", &history).await, "deleted manifest");

    // A manifest read error other than a missing file is a server error; the
    // request is not forwarded. A directory at the manifest path fails the read.
    fs::create_dir(manifest_path(&dir, "session-r")).unwrap();
    let (status, body) = p
        .send(Some("session-r"), conversation("m", &[]))
        .await
        .unwrap_err();
    assert_eq!((status, &body["error"]["type"]), (500, &json!("api_error")));

    // A failed commit is a server error too. On unix a dangling symlink at the
    // sessions path makes the manifest read return NotFound, so the request
    // reaches the commit, and the directory creation fails for any uid, root
    // included. Elsewhere a regular file gives the same errors.
    let sessions = path.parent().unwrap();
    fs::remove_dir_all(sessions).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("missing"), sessions).unwrap();
    #[cfg(not(unix))]
    fs::write(sessions, b"").unwrap();
    let read = fs::read(manifest_path(&dir, "session-w")).unwrap_err();
    assert_eq!(
        read.kind(),
        std::io::ErrorKind::NotFound,
        "read arm skipped"
    );
    let (status, body) = p
        .send(Some("session-w"), conversation("m", &[]))
        .await
        .unwrap_err();
    assert_eq!((status, &body["error"]["type"]), (500, &json!("api_error")));
    assert!(!dir.join("missing").exists(), "commit created no directory");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn changed_non_text_block_is_a_digest_mismatch() {
    let image = |data: &str| json!({"type": "image", "source": {"type": "base64", "data": data}});
    let text = json!({"type": "text", "text": R1});
    let body = |content: Value| {
        let messages = json!([
            {"role": "user", "content": "inspect the data"},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "cat x"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": content}
            ]},
            {"role": "user", "content": "next"}
        ]);
        json!({"model": "m", "messages": messages})
    };
    for (sent, resent) in [
        (json!([text, image("a")]), json!([text, image("b")])),
        (json!([image("a")]), json!([image("b")])),
    ] {
        let dir = temp_dir();
        write_owner(&dir, Owner::Proxy).unwrap();
        let mut p = Proxy::start(&dir, Mode::Safe).await;
        p.turn("m", &[]).await.unwrap();
        let mut first = body(sent.clone());
        first["messages"].as_array_mut().unwrap().pop();
        p.send(Some(SCOPE), serde_json::to_vec(&first).unwrap())
            .await
            .unwrap();
        // Formatting is not part of the source identity; content is.
        p.send(Some(SCOPE), serde_json::to_vec_pretty(&body(sent)).unwrap())
            .await
            .unwrap();
        let got = p
            .send(Some(SCOPE), serde_json::to_vec(&body(resent)).unwrap())
            .await;
        assert_rejected(got, "changed block");
        fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn moved_cache_breakpoint_is_not_a_digest_mismatch() {
    let cache = json!({"type": "ephemeral"});
    let text = |cached: bool| {
        let mut b = json!({"type": "text", "text": R1});
        if cached {
            b["cache_control"] = cache.clone();
        }
        b
    };
    let image = |cached: bool| {
        let mut b = json!({"type": "image", "source": {"type": "base64", "data": "a"}});
        if cached {
            b["cache_control"] = cache.clone();
        }
        b
    };
    let body = |content: Value, next: bool| {
        let mut messages = vec![
            json!({"role": "user", "content": "inspect the data"}),
            json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "cat x"}}
            ]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": content}
            ]}),
        ];
        if next {
            messages.push(json!({"role": "user", "content": "next"}));
        }
        serde_json::to_vec(&json!({"model": "m", "messages": messages})).unwrap()
    };
    // History without cache_control members, for byte comparison.
    let history = |sent: &Bytes| {
        let mut req: Value = serde_json::from_slice(sent).unwrap();
        let mut messages = req["messages"].as_array().unwrap()[..3].to_vec();
        for block in messages[2]["content"][0]["content"].as_array_mut().unwrap() {
            block.as_object_mut().unwrap().remove("cache_control");
        }
        req["messages"] = Value::Array(messages);
        req
    };
    let dir = temp_dir();
    write_owner(&dir, Owner::Proxy).unwrap();
    let mut p = Proxy::start(&dir, Mode::Safe).await;
    p.turn("m", &[]).await.unwrap();
    let first = p
        .send(Some(SCOPE), body(json!([text(true), image(false)]), false))
        .await
        .unwrap();
    // The client moves the breakpoint from the text block to the image block.
    let moved = p
        .send(Some(SCOPE), body(json!([text(false), image(true)]), true))
        .await
        .unwrap();
    // Then drops it from the content entirely.
    let dropped = p
        .send(Some(SCOPE), body(json!([text(false), image(false)]), true))
        .await
        .unwrap();
    let first = history(&first);
    assert_eq!(
        first["messages"][2]["content"][0]["content"][0]["text"],
        min(R1)
    );
    assert_eq!(history(&moved), first);
    assert_eq!(history(&dropped), first);
    let moved: Value = serde_json::from_slice(&moved).unwrap();
    assert_eq!(
        moved["messages"][2]["content"][0]["content"][1]["cache_control"],
        cache
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn off_session_stays_off_after_mode_change() {
    let dir = temp_dir();
    write_owner(&dir, Owner::Proxy).unwrap();
    let mut off = Proxy::start(&dir, Mode::Off).await;
    off.turn("m", &[]).await.unwrap();
    assert_eq!(manifest(&dir)["mode"], "off");
    let with_result = conversation("m", &[("toolu_1", R1)]);
    let seen = off.turn("m", &[("toolu_1", R1)]).await.unwrap();
    assert_eq!(seen, with_result);
    // Configured off does not hide a lost manifest.
    assert_rejected(off.send(Some("session-lost"), with_result).await, "off");
    drop(off);

    let mut safe = Proxy::start(&dir, Mode::Safe).await;
    let later = [("toolu_1", R1), ("toolu_3", R3)];
    assert_eq!(
        safe.turn("m", &later).await.unwrap(),
        conversation("m", &later)
    );
    assert!(manifest(&dir)["decisions"].as_object().unwrap().is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn request_without_session_id_passes_through() {
    let dir = temp_dir();
    write_owner(&dir, Owner::Proxy).unwrap();
    let mut p = Proxy::start(&dir, Mode::Safe).await;
    let body = conversation("m", &[("toolu_1", R1)]);
    for scope in [None, Some("")] {
        assert_eq!(p.send(scope, body.clone()).await.unwrap(), body);
    }
    assert!(
        !dir.join("sessions").exists(),
        "no manifest without a scope"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn proxy_compresses_only_when_it_owns_the_session() {
    for owner in [Some("hook\n"), Some("none\n"), Some("garbage\n"), None] {
        let dir = temp_dir();
        if let Some(owner) = owner {
            fs::write(dir.join("owner"), owner).unwrap();
        }
        let mut p = Proxy::start(&dir, Mode::Safe).await;
        for results in [
            &[][..],
            &[("toolu_1", R1)],
            &[("toolu_1", R1), ("toolu_4", R4)],
        ] {
            let body = conversation("m", results);
            assert_eq!(p.turn("m", results).await.unwrap(), body, "{owner:?}");
        }
        assert!(!dir.join("sessions").exists(), "{owner:?}: no manifest");
        fs::remove_dir_all(dir).unwrap();
    }
}
