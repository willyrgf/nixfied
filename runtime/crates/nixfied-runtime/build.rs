fn main() {
    println!("cargo:rerun-if-changed=src/service/endpoint/macos_fd.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/service/endpoint/macos_fd.c")
            .flag("-std=c11")
            .warnings_into_errors(true)
            .compile("nixfied_macos_fd");
    }
}
