fn main() {
    if std::env::var("CARGO_FEATURE_IIDC").is_ok() {
        // Homebrew library paths (macOS)
        if cfg!(target_os = "macos") {
            println!("cargo:rustc-link-search=/opt/homebrew/lib"); // Apple Silicon
            println!("cargo:rustc-link-search=/usr/local/lib"); // Intel
        }
        println!("cargo:rustc-link-lib=dc1394");
    }
}
