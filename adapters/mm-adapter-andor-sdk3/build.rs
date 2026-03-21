fn main() {
    #[cfg(not(feature = "andor-sdk3"))]
    {
        return;
    }

    #[cfg(feature = "andor-sdk3")]
    build_andor();
}

#[cfg(feature = "andor-sdk3")]
fn build_andor() {
    use std::path::PathBuf;

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Allow override via environment variable.
    let sdk_root = std::env::var("ANDOR_SDK3_ROOT").ok().map(PathBuf::from);

    // Also check the reference source tree for atcore.h.
    let ref_include = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../mmCoreAndDevices/DeviceAdapters/AndorSDK3");

    let mut build = cc::Build::new();
    build.file("src/shim.c");

    if ref_include.exists() {
        build.include(&ref_include);
    }

    match target_os.as_str() {
        "linux" => {
            let root = sdk_root.unwrap_or_else(|| PathBuf::from("/usr/local/andor/sdk3"));
            build.include(root.join("include"));
            println!("cargo:rustc-link-search={}", root.join("lib").display());
            println!("cargo:rustc-link-lib=atcore");
            println!("cargo:rustc-link-lib=atutility");
        }
        "windows" => {
            let root = sdk_root.unwrap_or_else(|| {
                PathBuf::from(r"C:\Program Files\Andor SDK3")
            });
            build.include(root.join("include"));
            let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
            let lib_dir = if arch == "x86_64" { root.join("lib64") } else { root.join("lib32") };
            println!("cargo:rustc-link-search={}", lib_dir.display());
            println!("cargo:rustc-link-lib=atcore");
            println!("cargo:rustc-link-lib=atutility");
        }
        "macos" => {
            // Andor SDK3 is not officially supported on macOS.
            eprintln!("cargo:warning=Andor SDK3 is not officially supported on macOS");
            let root = sdk_root.unwrap_or_else(|| PathBuf::from("/usr/local/andor/sdk3"));
            build.include(root.join("include"));
            println!("cargo:rustc-link-search={}", root.join("lib").display());
            println!("cargo:rustc-link-lib=atcore");
        }
        _ => {
            eprintln!("cargo:warning=Andor SDK3: unknown target platform");
        }
    }

    build.compile("andor3_shim");
    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-env-changed=ANDOR_SDK3_ROOT");
}
