use std::path::PathBuf;

fn main() {
    build_andor_sdk3();
    build_jai();
    build_picam();
    build_spot();
    build_tsi();
    build_twain();
    build_iidc();
}

fn build_andor_sdk3() {
    if std::env::var("CARGO_FEATURE_ANDOR_SDK3").is_err() {
        return;
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let sdk_root = std::env::var("ANDOR_SDK3_ROOT").ok().map(PathBuf::from);

    let ref_include =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mmCoreAndDevices/DeviceAdapters/AndorSDK3");

    let mut build = cc::Build::new();
    build.file("src/adapters/andor_sdk3/shim.c");

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
            let root = sdk_root.unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Andor SDK3"));
            build.include(root.join("include"));
            let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
            let lib_dir = if arch == "x86_64" {
                root.join("lib64")
            } else {
                root.join("lib32")
            };
            println!("cargo:rustc-link-search={}", lib_dir.display());
            println!("cargo:rustc-link-lib=atcore");
            println!("cargo:rustc-link-lib=atutility");
        }
        "macos" => {
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
    println!("cargo:rerun-if-changed=src/adapters/andor_sdk3/shim.c");
    println!("cargo:rerun-if-env-changed=ANDOR_SDK3_ROOT");
}

fn build_jai() {
    if std::env::var("CARGO_FEATURE_JAI").is_err() {
        return;
    }

    let sdk_root = find_jai_sdk_root();

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("src/adapters/jai/shim.cpp")
        .include(format!("{}/Includes", sdk_root))
        .flag_if_supported("-std=c++14")
        .flag_if_supported("-Wno-deprecated-declarations");

    if cfg!(target_os = "macos") {
        build.flag_if_supported("-Wno-unknown-pragmas");
    }

    build.compile("jai_shim");

    let lib_dir = format!("{}/Libraries", sdk_root);
    println!("cargo:rustc-link-search=native={}", lib_dir);

    for lib in &[
        "PvBase",
        "PvDevice",
        "PvBuffer",
        "PvStream",
        "PvGenICam",
        "EbNetworkLib",
        "EbUSBLib",
        "PvTransmitter",
    ] {
        println!("cargo:rustc-link-lib={}", lib);
    }

    println!("cargo:rerun-if-env-changed=EBUS_SDK_ROOT");
    println!("cargo:rerun-if-changed=src/adapters/jai/shim.cpp");
}

fn find_jai_sdk_root() -> String {
    if let Ok(root) = std::env::var("EBUS_SDK_ROOT") {
        return root;
    }
    #[cfg(target_os = "macos")]
    {
        let base = "/opt/pleora/ebus_sdk";
        if let Ok(entries) = std::fs::read_dir(base) {
            let mut candidates: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with("Darwin-"))
                .collect();
            candidates.sort_by_key(|e| e.file_name());
            if let Some(last) = candidates.last() {
                return last.path().to_string_lossy().into_owned();
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let base = "/opt/pleora/ebus_sdk";
        if let Ok(entries) = std::fs::read_dir(base) {
            let mut candidates: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            candidates.sort_by_key(|e| e.file_name());
            if let Some(last) = candidates.last() {
                return last.path().to_string_lossy().into_owned();
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let win = r"C:\Program Files\Pleora Technologies Inc\eBUS SDK";
        if std::path::Path::new(win).exists() {
            return win.to_string();
        }
    }
    panic!("eBUS SDK not found. Set EBUS_SDK_ROOT to its root directory.");
}

fn build_picam() {
    if std::env::var("CARGO_FEATURE_PICAM").is_err() {
        return;
    }

    let mut build = cc::Build::new();
    build.file("src/adapters/picam/shim.c").warnings(false);

    #[cfg(target_os = "macos")]
    {
        let framework_root = std::env::var("PVCAM_ROOT")
            .unwrap_or_else(|_| "/Library/Frameworks/PICAM.framework".into());
        build.include(format!("{}/Headers", framework_root));
        build.compile("picam_shim");
        println!("cargo:rustc-link-lib=framework=PICAM");
        println!("cargo:rustc-link-search=framework=/Library/Frameworks");
    }
    #[cfg(target_os = "linux")]
    {
        let root = std::env::var("PVCAM_ROOT").unwrap_or_else(|_| "/usr/local".into());
        build.include(format!("{}/include/pvcam", root));
        build.compile("picam_shim");
        println!("cargo:rustc-link-search=native={}/lib", root);
        println!("cargo:rustc-link-lib=pvcam");
    }
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var("PVCAM_ROOT")
            .unwrap_or_else(|_| r"C:\Program Files\Princeton Instruments\PVCAM".into());
        build.include(format!("{}\\SDK\\inc", root));
        build.compile("picam_shim");
        println!("cargo:rustc-link-search=native={}\\SDK\\lib", root);
        println!("cargo:rustc-link-lib=pvcam32");
    }

    println!("cargo:rerun-if-env-changed=PVCAM_ROOT");
    println!("cargo:rerun-if-changed=src/adapters/picam/shim.c");
}

fn build_spot() {
    if std::env::var("CARGO_FEATURE_SPOT").is_err() {
        return;
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let sdk_root = std::env::var("SPOT_SDK_ROOT").ok().map(PathBuf::from);

    let mut build = cc::Build::new();
    build.file("src/adapters/spot/shim.c");

    match target_os.as_str() {
        "macos" => {
            let framework_base = sdk_root.unwrap_or_else(|| PathBuf::from("/Library/Frameworks"));
            let headers = framework_base.join("SpotCam.framework/Headers");
            if headers.exists() {
                build.include(&headers);
            }
            println!(
                "cargo:rustc-link-search=framework={}",
                framework_base.display()
            );
            println!("cargo:rustc-link-lib=framework=SpotCam");
        }
        "windows" => {
            if let Some(root) = sdk_root {
                build.include(root.join("include"));
                println!("cargo:rustc-link-search={}", root.join("lib").display());
                println!("cargo:rustc-link-lib=SpotCam");
            }
        }
        "linux" => {
            eprintln!("cargo:warning=SpotCam SDK is not available on Linux");
        }
        _ => {}
    }

    build.compile("spot_shim");
    println!("cargo:rerun-if-changed=src/adapters/spot/shim.c");
    println!("cargo:rerun-if-env-changed=SPOT_SDK_ROOT");
}

fn build_tsi() {
    if std::env::var("CARGO_FEATURE_TSI").is_err() {
        return;
    }

    let sdk_root = find_tsi_sdk_root();

    let mut build = cc::Build::new();
    for sub in &[
        "include",
        "includes",
        "SDK/include",
        "Scientific Camera Interfaces/SDK/Native Toolkit/include",
    ] {
        let p = format!("{}/{}", sdk_root, sub);
        if std::path::Path::new(&p).exists() {
            build.include(&p);
        }
    }
    build
        .file("src/adapters/tsi/shim.c")
        .warnings(false)
        .compile("tsi_shim");

    for sub in &[
        "lib",
        "libs",
        "SDK/lib",
        "Scientific Camera Interfaces/SDK/Native Toolkit/lib",
    ] {
        let p = format!("{}/{}", sdk_root, sub);
        if std::path::Path::new(&p).exists() {
            println!("cargo:rustc-link-search=native={}", p);
        }
    }

    println!("cargo:rustc-link-lib=tl_camera_sdk");
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=pthread");

    println!("cargo:rerun-if-env-changed=TSI_SDK_ROOT");
    println!("cargo:rerun-if-changed=src/adapters/tsi/shim.c");
}

fn find_tsi_sdk_root() -> String {
    if let Ok(root) = std::env::var("TSI_SDK_ROOT") {
        return root;
    }
    #[cfg(target_os = "macos")]
    {
        for c in &[
            "/Library/Application Support/Thorlabs/Scientific Camera SDK",
            "/usr/local/thorlabs/tsi_sdk",
        ] {
            if std::path::Path::new(c).exists() {
                return c.to_string();
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for c in &["/opt/thorlabs/tsi_sdk", "/usr/local/tsi_sdk"] {
            if std::path::Path::new(c).exists() {
                return c.to_string();
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let c = r"C:\Program Files\Thorlabs\Scientific Imaging\Scientific Camera SDK";
        if std::path::Path::new(c).exists() {
            return c.to_string();
        }
    }
    panic!("Thorlabs Scientific Camera SDK not found. Set TSI_SDK_ROOT.");
}

fn build_twain() {
    if std::env::var("CARGO_FEATURE_TWAIN").is_err() {
        return;
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let sdk_root = std::env::var("TWAIN_SDK_ROOT").ok().map(PathBuf::from);

    let ref_include = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("mmCoreAndDevices/DeviceAdapters/TwainCamera");

    let mut build = cc::Build::new();
    build.file("src/adapters/twain/shim.c");

    if ref_include.exists() {
        build.include(&ref_include);
    }

    match target_os.as_str() {
        "windows" => {
            println!("cargo:rustc-link-lib=user32");
            println!("cargo:rustc-link-lib=gdi32");
            if let Some(root) = sdk_root {
                build.include(root.join("include"));
            }
        }
        "linux" => {
            let inc = sdk_root
                .map(|r| r.join("include"))
                .unwrap_or_else(|| PathBuf::from("/usr/include"));
            build.include(inc);
            println!("cargo:rustc-link-lib=dl");
        }
        _ => {
            eprintln!("cargo:warning=TWAIN is not supported on this platform");
        }
    }

    build.compile("twain_shim");
    println!("cargo:rerun-if-changed=src/adapters/twain/shim.c");
    println!("cargo:rerun-if-env-changed=TWAIN_SDK_ROOT");
}

fn build_iidc() {
    if std::env::var("CARGO_FEATURE_IIDC").is_err() {
        return;
    }

    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-search=/opt/homebrew/lib");
        println!("cargo:rustc-link-search=/usr/local/lib");
    }
    println!("cargo:rustc-link-lib=dc1394");
}
