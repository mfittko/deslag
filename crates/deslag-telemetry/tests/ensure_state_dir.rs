//! One test per binary: it changes process environment variables.

use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
};

use deslag_telemetry::ensure_state_dir;

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn set_state_dir(path: &Path) {
    // SAFETY: this binary runs no other thread that reads the environment.
    unsafe { std::env::set_var("DESLAG_STATE_DIR", path) };
}

#[test]
fn creates_0700_fixes_own_dirs_and_refuses_foreign_or_non_dirs() {
    let root = std::env::temp_dir().join(format!("deslag-ensure-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    // A real directory owned by the current uid.
    let dir = root.join("state");
    set_state_dir(&dir);
    assert_eq!(ensure_state_dir().unwrap(), dir);
    assert_eq!(mode(&dir), 0o700);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    ensure_state_dir().unwrap();
    assert_eq!(mode(&dir), 0o700);

    // A read-only directory owned by the current uid: the check never writes
    // into it, so it passes, gets 0700, and holds no probe file.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    ensure_state_dir().unwrap();
    assert_eq!(mode(&dir), 0o700);
    assert!(
        fs::read_dir(&dir).unwrap().next().is_none(),
        "no probe left"
    );

    // A symlink to a directory owned by the current uid: the target gets 0700.
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o777)).unwrap();
    let link = root.join("link");
    symlink(&target, &link).unwrap();
    set_state_dir(&link);
    assert_eq!(ensure_state_dir().unwrap(), link);
    assert_eq!(mode(&target), 0o700);
    assert!(
        fs::read_dir(&target).unwrap().next().is_none(),
        "no probe left"
    );

    // A path that is not a directory, directly or through a symlink.
    let file = root.join("file");
    fs::write(&file, b"").unwrap();
    let file_link = root.join("file-link");
    symlink(&file, &file_link).unwrap();
    for path in [&file, &file_link] {
        set_state_dir(path);
        assert!(ensure_state_dir().is_err(), "{path:?}");
    }

    // Symlinks to directories of another uid: `/` (0755) and `/tmp` (1777).
    // Each path owned by the test uid (root, or a sandbox tmpfs) is skipped.
    let uid = fs::metadata(&file).unwrap().uid();
    for foreign in ["/", "/tmp"] {
        if fs::metadata(foreign).unwrap().uid() == uid {
            continue;
        }
        let before = mode(Path::new(foreign));
        let link = root.join(format!("foreign{}", foreign.len()));
        symlink(foreign, &link).unwrap();
        set_state_dir(&link);
        assert!(ensure_state_dir().is_err(), "{foreign}");
        assert_eq!(mode(Path::new(foreign)), before);
    }

    fs::remove_dir_all(&root).unwrap();
}
