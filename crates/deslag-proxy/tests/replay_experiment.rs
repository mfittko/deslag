//! Replay experiment for ADR 0006: deterministic replay with persisted per-result decisions.
//!
//! The replay layer below is experiment-only. It rewrites Messages request JSON before the
//! request goes through the real deslag-proxy to a local mock upstream. No live provider calls.

use std::{
    convert::Infallible,
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use bytes::Bytes;
use deslag_proxy::{Upstream, serve};
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

const SCOPE_HEADER: &str = "x-claude-code-session-id";

// ---- Experiment-only replay layer -------------------------------------------------------

/// Policy in force when a new result is first seen.
#[derive(Clone, Copy)]
struct Policy {
    version: u64,
    min_len: usize,
    transform_version: u64,
}

const POLICY_V1: Policy = Policy {
    version: 1,
    min_len: 40,
    transform_version: 1,
};
const POLICY_V2: Policy = Policy {
    version: 2,
    min_len: 10,
    transform_version: 2,
};

#[derive(Clone, Debug, PartialEq)]
enum ReplayError {
    /// The request carries tool results but the scope has no readable manifest.
    MissingManifest,
    /// A known tool_use_id arrived with different source bytes.
    DigestMismatch(String),
    /// A tool result outside the newest message was never decided.
    UndecidedHistory(String),
    /// A pinned decision names a transform id or version this binary does not keep.
    RetiredTransform(String),
    /// A pinned decision is neither `unchanged: true` nor `encoded` with a transform id and a
    /// u64 version.
    CorruptDecision(String),
}

struct Replay {
    dir: PathBuf,
    policy: Policy,
    /// Simulated resource bypass: new results stay unchanged while set.
    overloaded: bool,
}

// ponytail: FNV-1a 64 is stable across toolchains but not collision resistant; production
// manifests need a cryptographic digest.
fn digest(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

// ponytail: toy transform standing in for the first real one. v1 minifies JSON; v2 adds a
// header line so the two versions produce different bytes for the upgrade scenario.
fn transform(version: u64, content: &str) -> Option<String> {
    let min = serde_json::to_string(&serde_json::from_str::<Value>(content).ok()?).ok()?;
    match version {
        1 => Some(min),
        2 => Some(format!("json-min/2\n{min}")),
        _ => None,
    }
}

impl Replay {
    fn manifest_path(&self, scope: &str) -> PathBuf {
        self.dir.join(format!("{}.json", digest(scope.as_bytes())))
    }

    /// Returns the body to forward. Writes the manifest before returning a changed body.
    fn apply(&self, scope: Option<&str>, body: &[u8]) -> Result<Vec<u8>, ReplayError> {
        // ponytail: no trusted scope means passthrough; production adds the owner handshake.
        let Some(scope) = scope else {
            return Ok(body.to_vec());
        };
        let Ok(mut req) = serde_json::from_slice::<Value>(body) else {
            return Ok(body.to_vec());
        };
        let path = self.manifest_path(scope);
        let loaded = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            // A manifest without a `decisions` object is as corrupt as one that does not parse.
            .filter(|m| m["decisions"].is_object());
        let has_results = !tool_results(&mut req).is_empty();
        let mut manifest = match loaded {
            Some(m) => m,
            None if has_results => return Err(ReplayError::MissingManifest),
            None => json!({
                "scope_digest": digest(scope.as_bytes()),
                "owner": "proxy",
                "mode": "safe",
                "model_profile": req["model"],
                "policy_version": self.policy.version,
                "decisions": {},
            }),
        };
        let mut dirty = !path.exists();
        let last = req["messages"]
            .as_array()
            .map_or(0, |m| m.len().saturating_sub(1));
        for (msg_idx, block) in tool_results(&mut req) {
            let (Some(id), Some(content)) = (
                block["tool_use_id"].as_str().map(str::to_owned),
                block["content"].as_str().map(str::to_owned),
            ) else {
                // ponytail: block-array and media content pass through undecided.
                continue;
            };
            let source = digest(content.as_bytes());
            let decision = match manifest["decisions"].get(&id) {
                Some(d) if d["digest"] != source.as_str() => {
                    return Err(ReplayError::DigestMismatch(id));
                }
                Some(d) => d.clone(),
                None if msg_idx != last => return Err(ReplayError::UndecidedHistory(id)),
                None => {
                    let d = self.decide(&content, &source);
                    manifest["decisions"][&id] = d.clone();
                    dirty = true;
                    d
                }
            };
            if let Some(version) = pinned_version(&decision, &id)? {
                // A retained transform version always succeeds on bytes it accepted before.
                let Some(encoded) = transform(version, &content) else {
                    return Err(ReplayError::RetiredTransform(id));
                };
                block["content"] = encoded.into();
            }
        }
        if dirty {
            write_atomic(&path, &serde_json::to_vec(&manifest).unwrap());
        }
        // ponytail: re-serialization canonicalizes key order and whitespace for every request,
        // so it is deterministic; production must splice changed blocks into the original bytes.
        Ok(serde_json::to_vec(&req).unwrap())
    }

    fn decide(&self, content: &str, source: &str) -> Value {
        let p = self.policy;
        let encoded = !self.overloaded
            && content.len() >= p.min_len
            && transform(p.transform_version, content).is_some_and(|t| t.len() < content.len());
        if encoded {
            json!({"digest": source, "policy_version": p.version,
                   "encoded": {"transform": "json-min", "version": p.transform_version}})
        } else {
            json!({"digest": source, "policy_version": p.version, "unchanged": true})
        }
    }
}

/// The pinned json-min version of an encoded decision, or `None` for `unchanged: true`.
/// Any other shape is an explicit error, never a silent replay as unchanged.
fn pinned_version(decision: &Value, id: &str) -> Result<Option<u64>, ReplayError> {
    if decision["unchanged"] == true && decision.get("encoded").is_none() {
        return Ok(None);
    }
    let encoded = &decision["encoded"];
    match (encoded["transform"].as_str(), encoded["version"].as_u64()) {
        (Some("json-min"), Some(version)) => Ok(Some(version)),
        (Some(_), Some(_)) => Err(ReplayError::RetiredTransform(id.to_owned())),
        _ => Err(ReplayError::CorruptDecision(id.to_owned())),
    }
}

/// String or block tool_result entries in any message, with their message index.
fn tool_results(req: &mut Value) -> Vec<(usize, &mut Value)> {
    let Some(messages) = req["messages"].as_array_mut() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, msg) in messages.iter_mut().enumerate() {
        if let Some(blocks) = msg["content"].as_array_mut() {
            for b in blocks.iter_mut().filter(|b| b["type"] == "tool_result") {
                out.push((i, b));
            }
        }
    }
    out
}

// ponytail: rename without fsync; production manifests need fsync of the file and its directory.
fn write_atomic(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).unwrap();
    fs::rename(&tmp, path).unwrap();
}

// ---- Harness: real proxy, mock upstream -------------------------------------------------

/// Mock upstream: records every request body and answers 200.
async fn mock() -> (SocketAddr, mpsc::UnboundedReceiver<Bytes>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let tx = tx.clone();
            let svc = service_fn(move |req: Request<Incoming>| {
                let tx = tx.clone();
                async move {
                    let body = req.into_body().collect().await.unwrap().to_bytes();
                    let _ = tx.send(body);
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"{}"))))
                }
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(stream), svc));
        }
    });
    (addr, rx)
}

/// One proxy process lifetime. Dropping it and building a new one simulates a restart.
struct Session {
    replay: Replay,
    proxy: SocketAddr,
    seen: mpsc::UnboundedReceiver<Bytes>,
}

impl Session {
    async fn start(dir: &Path, policy: Policy) -> Self {
        let (upstream, seen) = mock().await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = listener.local_addr().unwrap();
        let upstream = Upstream::parse(&format!("http://{upstream}")).unwrap();
        tokio::spawn(serve(listener, upstream));
        let replay = Replay {
            dir: dir.to_path_buf(),
            policy,
            overloaded: false,
        };
        Self {
            replay,
            proxy,
            seen,
        }
    }

    /// Applies replay, sends through the proxy, returns the body the upstream saw.
    async fn turn(&mut self, results: &[(&str, &str)]) -> Result<Bytes, ReplayError> {
        let body = self
            .replay
            .apply(Some("session-a"), &conversation(results))?;
        let stream = TcpStream::connect(self.proxy).await.unwrap();
        let (mut tx, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .unwrap();
        tokio::spawn(conn);
        let req = Request::post("/v1/messages")
            .header(SCOPE_HEADER, "session-a")
            .body(Full::new(Bytes::from(body.clone())))
            .unwrap();
        let resp = tx.send_request(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let seen = self.seen.recv().await.unwrap();
        assert_eq!(seen, body, "proxy must forward the replayed body unchanged");
        Ok(seen)
    }
}

/// A Messages request: one prompt, then a tool_use/tool_result pair per result.
fn conversation(results: &[(&str, &str)]) -> Vec<u8> {
    let mut messages = vec![json!({"role": "user", "content": "inspect the data"})];
    for (id, content) in results {
        messages.push(json!({"role": "assistant", "content": [
            {"type": "tool_use", "id": id, "name": "Bash", "input": {"command": "cat data.json"}}
        ]}));
        messages.push(json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": id, "content": content}
        ]}));
    }
    serde_json::to_vec(&json!({"model": "m", "max_tokens": 64, "messages": messages})).unwrap()
}

/// The upstream body of turn N+1 starts with every message byte of turn N.
fn assert_prefix_stable(earlier: &Bytes, later: &Bytes) {
    // Keys serialize sorted, so `max_tokens` and `messages` precede `model`.
    let tail = br#"],"model":"m"}"#;
    assert!(earlier.ends_with(tail));
    let prefix = &earlier[..earlier.len() - tail.len()];
    assert!(
        later.starts_with(prefix),
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

fn temp_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "deslag-replay-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    // Start empty: a panicked earlier run with the same pid may have left a manifest here.
    match fs::remove_dir_all(&dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => panic!("clear {dir:?}: {e}"),
        _ => {}
    }
    fs::create_dir(&dir).unwrap();
    dir
}

const R1: &str =
    "{ \"rows\": [ { \"id\": 1, \"name\": \"alpha\" }, { \"id\": 2, \"name\": \"beta\" } ] }";
const R2: &str = "{ \"ok\": true }";
const R3: &str = "{ \"items\": [ 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14 ] }";
/// Below the v1 threshold, but v2 output is shorter than the source.
const R4: &str = "{ \"a\" : 1 , \"b\" : 2 , \"c\" : 3 }";

// ---- Scenarios --------------------------------------------------------------------------

#[tokio::test]
async fn multi_turn_resend_and_restart_keep_prefix_byte_stable() {
    let dir = temp_dir();
    let mut s = Session::start(&dir, POLICY_V1).await;
    let t1 = s.turn(&[]).await.unwrap();
    let t2 = s.turn(&[("toolu_1", R1)]).await.unwrap();
    let t3 = s.turn(&[("toolu_1", R1), ("toolu_2", R2)]).await.unwrap();
    // Resending an identical history yields identical bytes.
    assert_eq!(
        s.turn(&[("toolu_1", R1), ("toolu_2", R2)]).await.unwrap(),
        t3
    );
    drop(s);

    // Restart: new replay layer and proxy; only the manifest on disk survives.
    let mut s = Session::start(&dir, POLICY_V1).await;
    let t4 = s
        .turn(&[("toolu_1", R1), ("toolu_2", R2), ("toolu_3", R3)])
        .await
        .unwrap();

    for (a, b) in [(&t1, &t2), (&t2, &t3), (&t3, &t4)] {
        assert_prefix_stable(a, b);
    }
    assert_eq!(content_of(&t4, "toolu_1"), transform(1, R1).unwrap());
    assert_eq!(
        content_of(&t4, "toolu_2"),
        R2,
        "below min_len stays unchanged"
    );
    assert_eq!(content_of(&t4, "toolu_3"), transform(1, R3).unwrap());
    println!(
        "replay: 4 turns + restart, upstream body bytes {:?}, toolu_1 {} -> {} bytes",
        [t1.len(), t2.len(), t3.len(), t4.len()],
        R1.len(),
        content_of(&t4, "toolu_1").len()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn policy_upgrade_replays_pinned_decisions() {
    let dir = temp_dir();
    let mut s = Session::start(&dir, POLICY_V1).await;
    // History with tool results and no manifest is never adopted silently.
    let before = s.turn(&[("toolu_1", R1), ("toolu_2", R4)]).await;
    assert!(matches!(before, Err(ReplayError::MissingManifest)));
    s.turn(&[]).await.unwrap();
    s.turn(&[("toolu_1", R1)]).await.unwrap();
    let v1 = s.turn(&[("toolu_1", R1), ("toolu_2", R4)]).await.unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(s.replay.manifest_path("session-a")).unwrap()).unwrap();
    assert_eq!(
        manifest["decisions"]["toolu_2"]["unchanged"], true,
        "premise: v1 left toolu_2 unchanged"
    );
    drop(s);

    // Premise: a fresh v2 decision on the same bytes encodes them.
    let fresh = Replay {
        dir: dir.clone(),
        policy: POLICY_V2,
        overloaded: false,
    };
    assert_eq!(fresh.decide(R4, "")["encoded"]["version"], 2);

    // Upgrade: new policy version, lower threshold, new transform version.
    let mut s = Session::start(&dir, POLICY_V2).await;
    let v2 = s
        .turn(&[("toolu_1", R1), ("toolu_2", R4), ("toolu_3", R3)])
        .await
        .unwrap();
    assert_prefix_stable(&v1, &v2);
    assert_eq!(content_of(&v2, "toolu_1"), transform(1, R1).unwrap());
    assert_eq!(
        content_of(&v2, "toolu_2"),
        R4,
        "v2 would encode it; v1 did not"
    );
    assert_eq!(content_of(&v2, "toolu_3"), transform(2, R3).unwrap());
    println!("replay: upgrade v1->v2 kept toolu_1@v1 and toolu_2 unchanged; toolu_3 used v2");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn first_unchanged_decision_stays_unchanged() {
    let dir = temp_dir();
    let mut s = Session::start(&dir, POLICY_V1).await;
    s.turn(&[]).await.unwrap();
    s.replay.overloaded = true;
    let loaded = s.turn(&[("toolu_1", R1)]).await.unwrap();
    s.replay.overloaded = false;
    let calm = s.turn(&[("toolu_1", R1), ("toolu_3", R3)]).await.unwrap();
    assert_prefix_stable(&loaded, &calm);
    assert_eq!(content_of(&calm, "toolu_1"), R1);
    assert_eq!(content_of(&calm, "toolu_3"), transform(1, R3).unwrap());
    println!("replay: toolu_1 left unchanged under load stayed unchanged after load dropped");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn manifest_loss_is_an_explicit_error() {
    let dir = temp_dir();
    let mut s = Session::start(&dir, POLICY_V1).await;
    s.turn(&[]).await.unwrap();
    s.turn(&[("toolu_1", R1)]).await.unwrap();
    let path = s.replay.manifest_path("session-a");

    fs::write(&path, b"{ not json").unwrap();
    let corrupt = s.turn(&[("toolu_1", R1), ("toolu_2", R2)]).await;
    assert_eq!(corrupt, Err(ReplayError::MissingManifest));

    // Parseable but wrong shape: no `decisions` object.
    for shape in [&b"[]"[..], b"{}", b"\"x\""] {
        fs::write(&path, shape).unwrap();
        let wrong = s.turn(&[("toolu_1", R1), ("toolu_2", R2)]).await;
        assert_eq!(wrong, Err(ReplayError::MissingManifest));
    }

    // A pinned transform version the binary does not keep.
    let retired = json!({"decisions": {"toolu_1": {"digest": digest(R1.as_bytes()),
        "policy_version": 1, "encoded": {"transform": "json-min", "version": 99}}}});
    fs::write(&path, serde_json::to_vec(&retired).unwrap()).unwrap();
    let unknown = s.turn(&[("toolu_1", R1), ("toolu_2", R2)]).await;
    assert_eq!(
        unknown,
        Err(ReplayError::RetiredTransform("toolu_1".into()))
    );

    fs::remove_file(&path).unwrap();
    let lost = s.turn(&[("toolu_1", R1), ("toolu_2", R2)]).await;
    assert_eq!(lost, Err(ReplayError::MissingManifest));
    println!("replay: corrupt and missing manifest -> {lost:?}");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn digest_mismatch_and_undecided_history_are_explicit_errors() {
    let dir = temp_dir();
    let mut s = Session::start(&dir, POLICY_V1).await;
    s.turn(&[]).await.unwrap();
    s.turn(&[("toolu_1", R1)]).await.unwrap();

    let changed = s.turn(&[("toolu_1", R3), ("toolu_2", R2)]).await;
    assert_eq!(changed, Err(ReplayError::DigestMismatch("toolu_1".into())));

    let skipped = s
        .turn(&[("toolu_1", R1), ("toolu_9", R3), ("toolu_2", R2)])
        .await;
    assert_eq!(
        skipped,
        Err(ReplayError::UndecidedHistory("toolu_9".into()))
    );
    println!("replay: {changed:?}; {skipped:?}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pinned_decision_forms_are_validated() {
    let replay = Replay {
        dir: temp_dir(),
        policy: POLICY_V1,
        overloaded: false,
    };
    let body = conversation(&[("toolu_1", R1)]);
    let retired = Err(ReplayError::RetiredTransform("toolu_1".into()));
    let corrupt = Err(ReplayError::CorruptDecision("toolu_1".into()));
    let cases: [(Value, Result<String, ReplayError>); 10] = [
        (json!({"unchanged": true}), Ok(R1.into())),
        (
            json!({"encoded": {"transform": "json-min", "version": 1}}),
            Ok(transform(1, R1).unwrap()),
        ),
        (
            json!({"encoded": {"transform": "json-min", "version": 2}}),
            Ok(transform(2, R1).unwrap()),
        ),
        (
            json!({"encoded": {"transform": "other", "version": 1}}),
            retired,
        ),
        (json!({"encoded": {"version": 1}}), corrupt.clone()),
        (
            json!({"encoded": {"transform": "json-min", "version": 1.0}}),
            corrupt.clone(),
        ),
        (
            json!({"encoded": {"transform": "json-min", "version": "1"}}),
            corrupt.clone(),
        ),
        (
            json!({"encoded": {"transform": "json-min", "version": -1}}),
            corrupt.clone(),
        ),
        (json!({}), corrupt.clone()),
        (json!({"unchanged": false}), corrupt),
    ];
    let path = replay.manifest_path("session-a");
    for (form, want) in &cases {
        let mut decision = form.clone();
        decision["digest"] = digest(R1.as_bytes()).into();
        let manifest = json!({"decisions": {"toolu_1": decision}});
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let got = replay
            .apply(Some("session-a"), &body)
            .map(|b| content_of(&Bytes::from(b), "toolu_1"));
        assert_eq!(&got, want, "decision {form}");
    }
    println!("replay: 10 pinned decision forms, 3 replayed, 7 explicit errors");
    fs::remove_dir_all(&replay.dir).unwrap();
}

#[test]
fn no_scope_passes_bytes_through() {
    let replay = Replay {
        dir: temp_dir(),
        policy: POLICY_V1,
        overloaded: false,
    };
    let body = conversation(&[("toolu_1", R1)]);
    assert_eq!(replay.apply(None, &body).unwrap(), body);
    assert!(
        fs::read_dir(&replay.dir).unwrap().next().is_none(),
        "no manifest without a scope"
    );
    fs::remove_dir_all(&replay.dir).unwrap();
}
