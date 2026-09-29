#![cfg(target_os = "macos")]

mod common;
use common::TempDir;

#[test]
fn sdk_decoder_rejects_denied_short_and_incoherent_socket_records() {
    let temp = TempDir::new();
    let binary = temp.path.join("decoder");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/test_macos_fd_decoder.c");
    let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let compiled = std::process::Command::new(compiler)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert!(
        std::process::Command::new(binary)
            .status()
            .unwrap()
            .success()
    );
}
