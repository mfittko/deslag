//! One test per binary: it changes process environment variables.

use std::path::PathBuf;

use deslag_telemetry::state_dir;

#[test]
fn relative_env_values_fall_through_to_the_next_source() {
    // SAFETY: this binary runs no other thread that reads the environment.
    unsafe {
        std::env::set_var("DESLAG_STATE_DIR", "rel/state");
        std::env::set_var("XDG_STATE_HOME", "/abs/xdg");
        std::env::set_var("HOME", "/abs/home");
    }
    assert_eq!(state_dir(), Some(PathBuf::from("/abs/xdg/deslag")));
    unsafe { std::env::set_var("XDG_STATE_HOME", "rel/xdg") };
    assert_eq!(
        state_dir(),
        Some(PathBuf::from("/abs/home/.local/state/deslag"))
    );
    unsafe { std::env::set_var("HOME", "") };
    assert_eq!(state_dir(), None);
    unsafe { std::env::set_var("DESLAG_STATE_DIR", "/abs/state") };
    assert_eq!(state_dir(), Some(PathBuf::from("/abs/state")));
}
