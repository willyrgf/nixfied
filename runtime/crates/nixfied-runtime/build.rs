fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    println!("cargo:rerun-if-env-changed=SDKROOT");
    let sdk = std::path::PathBuf::from(
        std::env::var_os("SDKROOT").expect("macOS builds require the Nix-selected SDKROOT"),
    );
    assert!(sdk.is_absolute(), "SDKROOT must be an absolute SDK path");
    let header = sdk.join("usr/include/sys/proc_info.h");
    assert!(header.is_file(), "selected SDK is missing sys/proc_info.h");
    println!("cargo:rerun-if-changed={}", header.display());
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
        .join("macos_socket_bindings.rs");
    // Nix supplies the unwrapped bindgen CLI: no host include-path wrapper may
    // override this SDK. Clang owns every record layout and emits compile-time
    // Rust size/alignment/offset assertions alongside the bindings.
    let status = std::process::Command::new("bindgen")
        .arg(header)
        .args([
            "--allowlist-type",
            "socket_fdinfo",
            "--allowlist-var",
            "PROC_PIDFDSOCKETINFO|SOCKINFO_TCP|TSI_S_LISTEN|INI_IPV4|INI_IPV6",
            "--rust-target",
            "1.85",
            "--rust-edition",
            "2024",
            "--formatter",
            "none",
            "--no-doc-comments",
            "--output",
        ])
        .arg(output)
        .arg("--")
        .arg(format!("--target={}", std::env::var("TARGET").unwrap()))
        .arg("-isysroot")
        .arg(sdk)
        .status()
        .expect("macOS builds require the Nix-packaged bindgen CLI");
    assert!(
        status.success(),
        "selected SDK socket binding generation failed"
    );
}
