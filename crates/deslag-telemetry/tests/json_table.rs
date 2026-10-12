//! Own binary: a live send resets the process-global drop counter that
//! sender.rs asserts on.

use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;

use deslag_core::FIRST_SLICE_JSON_MINIFY_CONTEXT;
use deslag_telemetry::{Adapter, ToolCategory, compress_reported};

#[test]
fn json_table_output_does_not_depend_on_the_service() {
    let dir = PathBuf::from(format!("/tmp/dsl-table-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.sock");
    let _receiver = UnixDatagram::bind(&path).unwrap();
    let mut ctx = FIRST_SLICE_JSON_MINIFY_CONTEXT;
    ctx.json_table = true;
    // Saves bytes but fewer tokens than the table token gate asks for.
    let input = r#"[{"id":1,"name":"alpha"},{"id":2,"name":"beta"},{"id":3,"name":"gamma"},{"id":4,"name":"delta"}]"#;
    let run = |socket| {
        let r = compress_reported(input, &ctx, socket, Adapter::Proxy, ToolCategory::Bash);
        (r.output.into_owned(), r.report.transform_id)
    };
    let plain = run(None);
    assert_eq!(
        plain.1, "json-table",
        "the table encoder applies without a socket"
    );
    assert_eq!(run(Some(&path)), plain);
    let _ = std::fs::remove_dir_all(&dir);
}
