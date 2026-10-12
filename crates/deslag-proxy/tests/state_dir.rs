//! One test per binary: it changes process environment variables.

use std::path::PathBuf;

use deslag_proxy::session::state_dir;

#[test]
fn state_dir_precedence_and_empty_or_relative_values_fall_through() {
    // SAFETY: this binary runs no other thread that reads the environment.
    let set = |k, v| unsafe { std::env::set_var(k, v) };
    set("DESLAG_STATE_DIR", "/abs/dir");
    set("XDG_STATE_HOME", "/abs/xdg");
    set("HOME", "/abs/home");
    assert_eq!(state_dir(), Some(PathBuf::from("/abs/dir")));
    set("DESLAG_STATE_DIR", "");
    assert_eq!(state_dir(), Some(PathBuf::from("/abs/xdg/deslag")));
    set("DESLAG_STATE_DIR", "rel");
    assert_eq!(state_dir(), Some(PathBuf::from("/abs/xdg/deslag")));
    unsafe { std::env::remove_var("DESLAG_STATE_DIR") };
    assert_eq!(state_dir(), Some(PathBuf::from("/abs/xdg/deslag")));
    set("XDG_STATE_HOME", "rel");
    assert_eq!(
        state_dir(),
        Some(PathBuf::from("/abs/home/.local/state/deslag"))
    );
    unsafe { std::env::remove_var("XDG_STATE_HOME") };
    assert_eq!(
        state_dir(),
        Some(PathBuf::from("/abs/home/.local/state/deslag"))
    );
    set("HOME", "");
    assert_eq!(state_dir(), None);
}
