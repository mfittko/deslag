//! Session state: owner file, retention, atomic commits, commit latency and
//! pinned-decision validation, against the public `session` API.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant, SystemTime},
};

use deslag_core::{JSON_MINIFY_ID as ID, JSON_MINIFY_VERSION as V};
use deslag_proxy::session::{
    Candidate, Mode, Owner, RETENTION, ReplayError, Store, manifest_path, prune, read_owner,
    write_owner,
};
use serde_json::{Value, json};

fn temp_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "deslag-session-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn store(dir: &Path) -> Store {
    write_owner(dir, Owner::Proxy).unwrap();
    Store {
        dir: dir.to_path_buf(),
        mode: Mode::Safe,
    }
}

fn candidate<'a>(id: &'a str, text: &str) -> Candidate<'a> {
    Candidate {
        id,
        newest: true,
        source: serde_json::to_string(text).unwrap(),
        texts: vec![text.to_owned()],
    }
}

fn manifest(dir: &Path, scope: &str) -> Value {
    serde_json::from_slice(&fs::read(manifest_path(dir, scope)).unwrap()).unwrap()
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn owner_round_trips_and_unknown_content_is_none() {
    let dir = temp_dir().join("state");
    assert_eq!(read_owner(&dir), Owner::None, "missing owner file");
    for (owner, text) in [
        (Owner::Hook, "hook\n"),
        (Owner::Proxy, "proxy\n"),
        (Owner::None, "none\n"),
    ] {
        write_owner(&dir, owner).unwrap();
        assert_eq!(read_owner(&dir), owner);
        assert_eq!(fs::read_to_string(dir.join("owner")).unwrap(), text);
    }
    #[cfg(unix)]
    {
        assert_eq!(mode_of(&dir), 0o700);
        assert_eq!(mode_of(&dir.join("owner")), 0o600);
    }
    for unknown in ["Proxy\n", "hook proxy\n", "", "\u{0}"] {
        fs::write(dir.join("owner"), unknown).unwrap();
        assert_eq!(read_owner(&dir), Owner::None, "{unknown:?}");
    }
    fs::remove_dir_all(dir.parent().unwrap()).unwrap();
}

#[test]
fn prune_removes_only_expired_session_files() {
    let dir = temp_dir();
    assert_eq!(prune(&dir, RETENTION, SystemTime::now()).unwrap(), 0);
    let store = store(&dir);
    for scope in ["old", "new"] {
        store.replay(scope, "m", &[], |_| true).unwrap();
    }
    let old = manifest_path(&dir, "old");
    let stale_tmp = old.with_extension("tmp");
    fs::write(&stale_tmp, b"partial").unwrap();
    let expired = SystemTime::now() - RETENTION - Duration::from_secs(60);
    for path in [&old, &stale_tmp] {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(expired)
            .unwrap();
    }
    assert_eq!(prune(&dir, RETENTION, SystemTime::now()).unwrap(), 2);
    assert!(!old.exists() && !stale_tmp.exists());
    assert!(manifest_path(&dir, "new").exists());
    assert!(dir.join("owner").exists(), "prune touches sessions only");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn commits_record_every_decision_and_their_latency_is_measured() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let path = manifest_path(&dir, "s");

    const N: usize = 200;
    let mut times = Vec::with_capacity(N);
    for i in 0..N {
        let id = format!("toolu_{i}");
        let c = [candidate(&id, "{ \"a\": 1 }")];
        let start = Instant::now();
        assert_eq!(
            store.replay("s", "m", &c, |_| i % 2 == 0).unwrap(),
            [i % 2 == 0]
        );
        times.push(start.elapsed());
    }
    times.sort();
    println!(
        "session: {N} commits, p50 {:?}, max {:?}",
        times[N / 2],
        times[N - 1]
    );

    let m = manifest(&dir, "s");
    assert_eq!(m["decisions"].as_object().unwrap().len(), N);
    assert_eq!(m["decisions"]["toolu_0"]["encoded"]["version"], V);
    assert_eq!(m["decisions"]["toolu_1"]["unchanged"], true);
    let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    #[cfg(unix)]
    {
        assert_eq!(mode_of(path.parent().unwrap()), 0o700);
        assert_eq!(mode_of(&path), 0o600);
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn commits_leave_other_temp_files_alone() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let path = manifest_path(&dir, "s");
    // Temp files of another process or an interrupted commit.
    let foreign = [path.with_extension("tmp"), path.with_extension("1.0.tmp")];
    for tmp in &foreign {
        fs::write(tmp, b"{ torn").unwrap();
    }
    for i in 0..3 {
        let id = format!("toolu_{i}");
        store
            .replay("s", "m", &[candidate(&id, "x")], |_| false)
            .unwrap();
    }
    for tmp in &foreign {
        assert_eq!(fs::read(tmp).unwrap(), b"{ torn", "{tmp:?}");
    }
    assert_eq!(
        manifest(&dir, "s")["decisions"].as_object().unwrap().len(),
        3
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_crash_leftover_at_the_first_temp_name_does_not_fail_the_commit() {
    let dir = temp_dir();
    let store = store(&dir);
    let path = manifest_path(&dir, "s");
    // Left by a crashed proxy that had the same pid, for example pid 1. They
    // are planted before the first commit at the first temp names, so that
    // commit hits them.
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let leftovers: Vec<_> = (0..256)
        .map(|n| path.with_extension(format!("{}.{n}.tmp", std::process::id())))
        .collect();
    for leftover in &leftovers {
        fs::write(leftover, b"{ torn").unwrap();
    }
    store.replay("s", "m", &[], |_| true).unwrap();
    store
        .replay("s", "m", &[candidate("toolu_0", "x")], |_| false)
        .unwrap();
    for leftover in &leftovers {
        assert_eq!(fs::read(leftover).unwrap(), b"{ torn", "{leftover:?}");
    }
    assert_eq!(
        manifest(&dir, "s")["decisions"]["toolu_0"]["unchanged"],
        true
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unreadable_manifest_is_an_io_error() {
    let dir = temp_dir();
    let store = store(&dir);
    fs::create_dir_all(manifest_path(&dir, "s")).unwrap();
    for c in [vec![], vec![candidate("toolu_1", "x")]] {
        let got = store.replay("s", "m", &c, |_| true);
        assert!(matches!(got, Err(ReplayError::Io(_))), "{got:?}");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prune_skips_entries_that_are_not_files() {
    let dir = temp_dir();
    store(&dir).replay("s", "m", &[], |_| true).unwrap();
    let not_a_file = dir.join("sessions/stray.json");
    fs::create_dir(&not_a_file).unwrap();
    let later = SystemTime::now() + RETENTION + Duration::from_secs(60);
    assert_eq!(prune(&dir, RETENTION, later).unwrap(), 1);
    assert!(!manifest_path(&dir, "s").exists());
    assert!(not_a_file.is_dir());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_decision_that_is_not_an_object_is_corrupt() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let c = [candidate("toolu_1", "x")];
    store.replay("s", "m", &c, |_| true).unwrap();
    let base = manifest(&dir, "s");
    let digest = base["decisions"]["toolu_1"]["digest"].clone();
    for form in [json!(null), json!(true), digest.clone(), json!([digest])] {
        let mut m = base.clone();
        m["decisions"]["toolu_1"] = form.clone();
        fs::write(manifest_path(&dir, "s"), m.to_string()).unwrap();
        let got = store.replay("s", "m", &c, |_| unreachable!());
        assert!(
            matches!(&got, Err(ReplayError::CorruptDecision(id)) if id == "toolu_1"),
            "{form}: {got:?}"
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn decision_digest_forms_are_validated() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let c = [candidate("toolu_1", "x")];
    store.replay("s", "m", &c, |_| true).unwrap();
    let base = manifest(&dir, "s");
    let other = "0".repeat(64);
    for (digest, mismatch) in [
        (None, false),
        (Some(json!(null)), false),
        (Some(json!(123)), false),
        (Some(json!(other)), true),
    ] {
        let mut m = base.clone();
        let mut decision = json!({"unchanged": true});
        if let Some(d) = &digest {
            decision["digest"] = d.clone();
        }
        m["decisions"]["toolu_1"] = decision;
        fs::write(manifest_path(&dir, "s"), m.to_string()).unwrap();
        let got = store.replay("s", "m", &c, |_| unreachable!());
        let ok = match &got {
            Err(ReplayError::DigestMismatch(id)) => mismatch && id == "toolu_1",
            Err(ReplayError::CorruptDecision(id)) => !mismatch && id == "toolu_1",
            _ => false,
        };
        assert!(ok, "{digest:?}: {got:?}");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_corrupt_manifest_is_never_overwritten_without_tool_results() {
    let dir = temp_dir();
    let store = store(&dir);
    let path = manifest_path(&dir, "s");
    store.replay("s", "m", &[], |_| true).unwrap();
    let mut v2 = manifest(&dir, "s");
    v2["version"] = json!(2);
    for bytes in [
        b"{ not json".to_vec(),
        v2.to_string().into_bytes(),
        br#"{"owner":"proxy","mode":"safe","decisions":{}}"#.to_vec(),
    ] {
        fs::write(&path, &bytes).unwrap();
        let got = store.replay("s", "m", &[], |_| unreachable!());
        assert!(matches!(&got, Ok(v) if v.is_empty()), "{got:?}");
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pinned_decision_forms_are_validated() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let c = [candidate("toolu_1", "x")];
    store.replay("s", "m", &c, |_| true).unwrap();
    let base = manifest(&dir, "s");
    let digest = base["decisions"]["toolu_1"]["digest"].clone();
    assert_eq!(digest.as_str().unwrap().len(), 64, "sha-256 hex");

    let ok = |encode: bool| Some(encode);
    let cases: [(Value, Option<bool>, &str); 13] = [
        (json!({"unchanged": true}), ok(false), ""),
        (
            json!({"encoded": {"transform": ID, "version": V}}),
            ok(true),
            "",
        ),
        (
            json!({"encoded": {"transform": ID, "version": V + 1}}),
            None,
            "retired",
        ),
        (
            json!({"encoded": {"transform": "other", "version": 1}}),
            None,
            "retired",
        ),
        (
            json!({"encoded": {"transform": "json-min", "version": V}}),
            None,
            "retired",
        ),
        (json!({"encoded": {"version": 1}}), None, "corrupt"),
        (
            json!({"encoded": {"transform": ID, "version": 1.0}}),
            None,
            "corrupt",
        ),
        (
            json!({"encoded": {"transform": ID, "version": "1"}}),
            None,
            "corrupt",
        ),
        (
            json!({"encoded": {"transform": ID, "version": -1}}),
            None,
            "corrupt",
        ),
        (json!({}), None, "corrupt"),
        (json!({"unchanged": false}), None, "corrupt"),
        (
            json!({"unchanged": true, "encoded": {"transform": ID, "version": V}}),
            None,
            "corrupt",
        ),
        (
            json!({"unchanged": false, "encoded": {"transform": ID, "version": V}}),
            None,
            "corrupt",
        ),
    ];
    for (form, want, err) in cases {
        let mut m = base.clone();
        let mut decision = form.clone();
        decision["digest"] = digest.clone();
        m["decisions"]["toolu_1"] = decision;
        fs::write(manifest_path(&dir, "s"), m.to_string()).unwrap();
        // Replays never decide again, so `decide` must not be consulted.
        let got = store.replay("s", "m", &c, |_| unreachable!());
        match (got, want, err) {
            (Ok(v), Some(w), _) => assert_eq!(v, [w], "{form}"),
            (Err(ReplayError::RetiredTransform(id)), None, "retired")
            | (Err(ReplayError::CorruptDecision(id)), None, "corrupt") => {
                assert_eq!(id, "toolu_1")
            }
            (got, _, _) => panic!("{form}: {got:?}"),
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

fn old(c: Candidate<'_>) -> Candidate<'_> {
    Candidate { newest: false, ..c }
}

#[test]
fn without_a_manifest_hook_and_none_owners_pass_through_without_writes() {
    let dir = temp_dir();
    let c = [candidate("toolu_1", "{ \"a\": 1 }")];
    for owner in [Owner::Hook, Owner::None] {
        write_owner(&dir, owner).unwrap();
        let store = Store {
            dir: dir.clone(),
            mode: Mode::Safe,
        };
        for cs in [&c[..], &[]] {
            let got = store.replay("s", "m", cs, |_| unreachable!()).unwrap();
            assert_eq!(got, vec![false; cs.len()], "{owner:?}");
        }
        assert!(!manifest_path(&dir, "s").exists(), "{owner:?}");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn proxy_owner_with_tool_results_and_no_manifest_is_missing_manifest() {
    let dir = temp_dir();
    let got = store(&dir).replay("s", "m", &[candidate("toolu_1", "x")], |_| unreachable!());
    assert!(matches!(got, Err(ReplayError::MissingManifest)), "{got:?}");
    assert!(!manifest_path(&dir, "s").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn corrupt_manifest_with_tool_results_is_missing_manifest() {
    let dir = temp_dir();
    let store = store(&dir);
    let path = manifest_path(&dir, "s");
    store.replay("s", "m", &[], |_| true).unwrap();
    let mut array = manifest(&dir, "s");
    array["decisions"] = json!([]);
    let corrupt = [
        b"{ not json".to_vec(),
        array.to_string().into_bytes(),
        b"[]".to_vec(),
        b"{}".to_vec(),
        br#"{"version":1}"#.to_vec(),
        br#"{"owner":"proxy","mode":"safe","decisions":{}}"#.to_vec(),
    ];
    // A corrupt manifest is an error under every owner, never a passthrough.
    for owner in [Owner::Proxy, Owner::Hook, Owner::None] {
        write_owner(&dir, owner).unwrap();
        for bytes in &corrupt {
            fs::write(&path, bytes).unwrap();
            let got = store.replay("s", "m", &[candidate("toolu_1", "x")], |_| unreachable!());
            assert!(
                matches!(got, Err(ReplayError::MissingManifest)),
                "{owner:?}: {got:?}"
            );
            assert_eq!(&fs::read(&path).unwrap(), bytes, "{owner:?}");
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn version_2_manifest_with_tool_results_is_unsupported() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let mut m = manifest(&dir, "s");
    m["version"] = json!(2);
    // A future format may reshape or drop `decisions`; the version decides.
    for m in [
        m,
        json!({"version": 2, "decisions": []}),
        json!({"version": 2, "decisions": {}}),
        json!({"version": 2}),
    ] {
        fs::write(manifest_path(&dir, "s"), m.to_string()).unwrap();
        let got = store.replay("s", "m", &[candidate("toolu_1", "x")], |_| unreachable!());
        assert!(
            matches!(got, Err(ReplayError::UnsupportedManifest)),
            "{m}: {got:?}"
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_with_a_foreign_owner_or_no_mode_is_missing_manifest() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let base = manifest(&dir, "s");
    let mut hook = base.clone();
    hook["owner"] = json!("hook");
    let mut no_owner = base.clone();
    no_owner.as_object_mut().unwrap().remove("owner");
    let mut no_mode = base.clone();
    no_mode.as_object_mut().unwrap().remove("mode");
    for m in [hook, no_owner, no_mode] {
        fs::write(manifest_path(&dir, "s"), m.to_string()).unwrap();
        let got = store.replay("s", "m", &[candidate("toolu_1", "x")], |_| unreachable!());
        assert!(
            matches!(got, Err(ReplayError::MissingManifest)),
            "{m}: {got:?}"
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mode_off_session_passes_through_without_writes() {
    let dir = temp_dir();
    let store = Store {
        mode: Mode::Off,
        ..store(&dir)
    };
    store.replay("s", "m", &[], |_| true).unwrap();
    let path = manifest_path(&dir, "s");
    let before = fs::read(&path).unwrap();
    assert_eq!(manifest(&dir, "s")["mode"], "off");
    let c = [candidate("toolu_1", "{ \"a\": 1 }")];
    assert_eq!(
        store.replay("s", "m", &c, |_| unreachable!()).unwrap(),
        [false]
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    // The manifest mode is pinned: a configuration change to `safe` does not
    // reach the existing `off` session.
    let safe = Store {
        mode: Mode::Safe,
        ..store.clone()
    };
    assert_eq!(
        safe.replay("s", "m", &c, |_| unreachable!()).unwrap(),
        [false]
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mode_safe_session_keeps_deciding_after_a_change_to_off() {
    let dir = temp_dir();
    store(&dir).replay("s", "m", &[], |_| true).unwrap();
    let off = Store {
        mode: Mode::Off,
        ..store(&dir)
    };
    let c = [candidate("toolu_1", "{ \"a\": 1 }")];
    assert_eq!(off.replay("s", "m", &c, |_| true).unwrap(), [true]);
    let m = manifest(&dir, "s");
    assert_eq!(m["mode"], "safe");
    assert_eq!(m["decisions"]["toolu_1"]["encoded"]["transform"], ID);
    fs::remove_dir_all(dir).unwrap();
}

fn backdate(path: &Path) -> SystemTime {
    let t = SystemTime::now() - RETENTION - Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
    // The filesystem may truncate `t` to its timestamp granularity.
    mtime(path)
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn replay_without_a_commit_refreshes_retention_of_valid_manifests_only() {
    let dir = temp_dir();
    let safe = store(&dir);
    let off = Store {
        mode: Mode::Off,
        ..safe.clone()
    };
    off.replay("off", "m", &[], |_| true).unwrap();
    safe.replay("safe", "m", &[], |_| true).unwrap();
    safe.replay("idle", "m", &[], |_| true).unwrap();
    let c = [candidate("toolu_1", "x")];
    safe.replay("safe", "m", &c, |_| false).unwrap();
    let path = |s| manifest_path(&dir, s);
    let before = ["off", "safe"].map(|s| fs::read(path(s)).unwrap());
    for s in ["off", "safe", "idle"] {
        backdate(&path(s));
    }
    // Used sessions: an `off` session, and a `safe` one that only replays.
    off.replay("off", "m", &c, |_| unreachable!()).unwrap();
    assert_eq!(
        safe.replay("safe", "m", &c, |_| unreachable!()).unwrap(),
        [false]
    );
    assert_eq!(prune(&dir, RETENTION, SystemTime::now()).unwrap(), 1);
    assert!(!path("idle").exists(), "an idle session expires");
    for (s, bytes) in ["off", "safe"].iter().zip(&before) {
        assert_eq!(&fs::read(path(s)).unwrap(), bytes, "{s}: bytes unchanged");
    }
    // Corrupt and unsupported manifests are never refreshed.
    for bytes in [&b"{ not json"[..], br#"{"version":2,"decisions":{}}"#] {
        fs::write(path("bad"), bytes).unwrap();
        let old = backdate(&path("bad"));
        safe.replay("bad", "m", &[], |_| unreachable!()).unwrap();
        assert_eq!(mtime(&path("bad")), old);
    }
    // A scope without a manifest under a non-proxy owner gets none.
    write_owner(&dir, Owner::Hook).unwrap();
    safe.replay("none", "m", &[], |_| unreachable!()).unwrap();
    assert!(!path("none").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_manifest_that_cannot_be_refreshed_still_replays() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir();
    let safe = store(&dir);
    let off = Store {
        mode: Mode::Off,
        ..safe.clone()
    };
    off.replay("off", "m", &[], |_| true).unwrap();
    safe.replay("safe", "m", &[], |_| true).unwrap();
    let c = [candidate("toolu_1", "x")];
    safe.replay("safe", "m", &c, |_| false).unwrap();
    for s in ["off", "safe"] {
        let path = manifest_path(&dir, s);
        let old = backdate(&path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let store = if s == "off" { &off } else { &safe };
        let got = store.replay(s, "m", &c, |_| unreachable!()).unwrap();
        assert_eq!(got, [false], "{s}");
        // Skip the mtime check when running as root, which can write anyway.
        if fs::File::options().write(true).open(&path).is_err() {
            assert_eq!(mtime(&path), old, "{s}: mtime unchanged");
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prune_never_removes_a_manifest_that_a_concurrent_replay_refreshed() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let path = manifest_path(&dir, "s");
    let c = [candidate("toolu_1", "x")];
    for _ in 0..200 {
        backdate(&path);
        std::thread::scope(|s| {
            let pruner = s.spawn(|| prune(&dir, RETENTION, SystemTime::now()).unwrap());
            // Either prune runs first and this replay starts a new manifest,
            // or this replay refreshes the manifest and prune keeps it.
            store.replay("s", "m", &[], |_| true).unwrap();
            pruner.join().unwrap();
        });
        assert!(path.exists());
        store.replay("s", "m", &c, |_| false).unwrap();
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unknown_result_outside_the_newest_message_is_undecided_history() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let got = store.replay(
        "s",
        "m",
        &[old(candidate("toolu_1", "x"))],
        |_| unreachable!(),
    );
    assert!(
        matches!(&got, Err(ReplayError::UndecidedHistory(id)) if id == "toolu_1"),
        "{got:?}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn after_a_switch_to_hook_pinned_results_replay_and_new_ones_stay_unchanged() {
    let dir = temp_dir();
    let store = store(&dir);
    store.replay("s", "m", &[], |_| true).unwrap();
    let first = [candidate("toolu_1", "{ \"a\": 1 }")];
    assert_eq!(store.replay("s", "m", &first, |_| true).unwrap(), [true]);
    write_owner(&dir, Owner::Hook).unwrap();
    let c = [
        old(candidate("toolu_1", "{ \"a\": 1 }")),
        candidate("toolu_2", "{ \"b\": 2 }"),
    ];
    let got = store.replay("s", "m", &c, |_| panic!("decide under hook owner"));
    assert_eq!(got.unwrap(), [true, false]);
    let m = manifest(&dir, "s");
    assert_eq!(m["decisions"]["toolu_1"]["encoded"]["transform"], ID);
    assert_eq!(m["decisions"]["toolu_2"]["unchanged"], true);
    assert!(m["decisions"]["toolu_2"].get("encoded").is_none());
    fs::remove_dir_all(dir).unwrap();
}
