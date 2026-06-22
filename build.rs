use std::path::PathBuf;

fn main() {
    build_andor_sdk3();
    build_daheng();
    build_jai();
    build_picam();
    build_spot();
    build_tsi();
    build_twain();
    build_iidc();
}

fn build_daheng() {
    if std::env::var("CARGO_FEATURE_DAHENG").is_err() {
        return;
    }

    let use_stub = std::env::var("DAHENG_STUB").is_ok();
    if use_stub {
        cc::Build::new()
            .file("vendor/daheng_stub/gxiapi_stub.c")
            .warnings(false)
            .compile("gxiapi");
        println!("cargo:rerun-if-env-changed=DAHENG_STUB");
        println!("cargo:rerun-if-changed=vendor/daheng_stub/gxiapi_stub.c");
        return;
    }

    let root = std::env::var("DAHENG_SDK_ROOT")
        .or_else(|_| std::env::var("GALAXY_ROOT"))
        .ok()
        .map(PathBuf::from);

    if let Ok(lib_dir) = std::env::var("DAHENG_LIB_DIR") {
        println!("cargo:rustc-link-search=native={}", lib_dir);
    } else if let Some(root) = root {
        let arch = daheng_arch_dir();
        for sub in &[
            "lib",
            "lib64",
            "Libraries",
            "Bin",
            "bin",
            &format!("lib/{}", arch),
            &format!("lib/{}", if arch == "x86_64" { "x64" } else { arch }),
        ] {
            let p = root.join(sub);
            if p.exists() {
                println!("cargo:rustc-link-search=native={}", p.display());
            }
        }
    }

    println!("cargo:rustc-link-lib=gxiapi");
    println!("cargo:rerun-if-env-changed=DAHENG_STUB");
    println!("cargo:rerun-if-env-changed=DAHENG_SDK_ROOT");
    println!("cargo:rerun-if-env-changed=GALAXY_ROOT");
    println!("cargo:rerun-if-env-changed=DAHENG_LIB_DIR");
}

fn daheng_arch_dir() -> &'static str {
    match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x86_64",
        Ok("x86") => "x86",
        Ok("aarch64") => "aarch64",
        Ok("arm") => "armv7l",
        _ => "x86_64",
    }
}

fn build_andor_sdk3() {
    if std::env::var("CARGO_FEATURE_ANDOR_SDK3").is_err() {
        return;
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let sdk_root = std::env::var("ANDOR_SDK3_ROOT").ok().map(PathBuf::from);
    let use_stub = std::env::var("ANDOR_SDK3_STUB").is_ok();

    let ref_include =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mmCoreAndDevices/DeviceAdapters/AndorSDK3");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("src/adapters/andor_sdk3/shim.c")
        .flag_if_supported("-std=c++11");

    if use_stub {
        build
            .include("vendor/andor_sdk3_stub")
            .file("vendor/andor_sdk3_stub/andor_sdk3_stub.c");
        build.compile("andor3_shim");
        println!("cargo:rerun-if-env-changed=ANDOR_SDK3_STUB");
        println!("cargo:rerun-if-changed=src/adapters/andor_sdk3/shim.c");
        println!("cargo:rerun-if-changed=vendor/andor_sdk3_stub/atcore.h");
        println!("cargo:rerun-if-changed=vendor/andor_sdk3_stub/atutility.h");
        println!("cargo:rerun-if-changed=vendor/andor_sdk3_stub/andor_sdk3_stub.c");
        return;
    }

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
    println!("cargo:rerun-if-env-changed=ANDOR_SDK3_STUB");
    println!("cargo:rerun-if-env-changed=ANDOR_SDK3_ROOT");
}

fn build_jai() {
    if std::env::var("CARGO_FEATURE_JAI").is_err() {
        return;
    }

    let use_stub = std::env::var("JAI_STUB").is_ok();
    if use_stub {
        cc::Build::new()
            .cpp(true)
            .file("vendor/jai_stub/jai_stub.cpp")
            .flag_if_supported("-std=c++14")
            .warnings(false)
            .compile("jai_shim");
        println!("cargo:rerun-if-env-changed=JAI_STUB");
        println!("cargo:rerun-if-changed=vendor/jai_stub/jai_stub.cpp");
        return;
    }

    let sdk_root = find_jai_sdk_root();
    let include_dir = first_existing_dir(&sdk_root, &["Includes", "include"]);
    let lib_dir = first_existing_dir(&sdk_root, &["Libraries", "lib"]);

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("src/adapters/jai/shim.cpp")
        .include(&include_dir)
        .flag_if_supported("-std=c++14")
        .define("_UNIX_", None)
        .define("_LINUX_", None)
        .flag_if_supported("-Wno-deprecated-declarations");

    if cfg!(target_os = "macos") {
        build.flag_if_supported("-Wno-unknown-pragmas");
    }

    build.compile("jai_shim");

    println!("cargo:rustc-link-search=native={}", lib_dir);

    for lib in &[
        "PvAppUtils",
        "PvBase",
        "PvDevice",
        "PvBuffer",
        "PvStream",
        "PvGenICam",
        "PvSystem",
        "PvVirtualDevice",
        "PvPersistence",
        "PvSerial",
        "PvCameraBridge",
        "PtConvertersLib",
        "SimpleImagingLib",
        "EbTransportLayerLib",
        "EbUtilsLib",
        "EbNetworkLib",
        "EbUSBLib",
        "PvTransmitter",
    ] {
        if std::path::Path::new(&format!("{}/lib{}.so", lib_dir, lib)).exists()
            || std::path::Path::new(&format!("{}/lib{}.dylib", lib_dir, lib)).exists()
            || std::path::Path::new(&format!("{}/{}.lib", lib_dir, lib)).exists()
        {
            println!("cargo:rustc-link-lib={}", lib);
        }
    }

    println!("cargo:rerun-if-env-changed=EBUS_SDK_ROOT");
    println!("cargo:rerun-if-env-changed=JAI_STUB");
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
        for base in ["/opt/jai/ebus_sdk", "/opt/pleora/ebus_sdk"] {
            if let Ok(entries) = std::fs::read_dir(base) {
                let mut candidates: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                candidates.sort_by_key(|e| e.file_name());
                if let Some(last) = candidates.last() {
                    return last.path().to_string_lossy().into_owned();
                }
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

fn first_existing_dir(root: &str, names: &[&str]) -> String {
    for name in names {
        let path = format!("{}/{}", root, name);
        if std::path::Path::new(&path).is_dir() {
            return path;
        }
    }
    format!("{}/{}", root, names[0])
}

fn build_picam() {
    if std::env::var("CARGO_FEATURE_PICAM").is_err() {
        return;
    }

    let mut build = cc::Build::new();
    build.file("src/adapters/picam/shim.c").warnings(false);
    let use_stub = std::env::var("PVCAM_STUB").is_ok();

    if use_stub {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/pvcam_stub");
        build.include(root.join("include"));
        build.file(root.join("pvcam_stub.c"));
        build.compile("picam_shim");
        println!("cargo:rerun-if-env-changed=PVCAM_STUB");
        println!("cargo:rerun-if-changed=src/adapters/picam/shim.c");
        println!("cargo:rerun-if-changed=vendor/pvcam_stub/include/pvcam/master.h");
        println!("cargo:rerun-if-changed=vendor/pvcam_stub/include/pvcam/pvcam.h");
        println!("cargo:rerun-if-changed=vendor/pvcam_stub/pvcam_stub.c");
        return;
    }

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
        let root = std::env::var("PVCAM_ROOT").unwrap_or_else(|_| "/opt/pvcam".into());
        let arch = pvcam_arch_dir();
        for include_dir in [
            format!("{}/include", root),
            format!("{}/include/pvcam", root),
            format!("{}/sdk/include", root),
            format!("{}/sdk/include/pvcam", root),
        ] {
            if std::path::Path::new(&include_dir).is_dir() {
                build.include(include_dir);
            }
        }
        build.compile("picam_shim");
        let lib_dir = std::env::var("PVCAM_LIB_DIR")
            .ok()
            .or_else(|| {
                [
                    format!("{}/lib", root),
                    format!("{}/library/{}", root, arch),
                    format!("{}/sdk/library/{}", root, arch),
                ]
                .into_iter()
                .find(|p| std::path::Path::new(p).is_dir())
            })
            .unwrap_or_else(|| format!("{}/lib", root));
        println!("cargo:rustc-link-search=native={}", lib_dir);
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
    println!("cargo:rerun-if-env-changed=PVCAM_LIB_DIR");
    println!("cargo:rerun-if-env-changed=PVCAM_STUB");
    println!("cargo:rerun-if-changed=src/adapters/picam/shim.c");
}

fn pvcam_arch_dir() -> &'static str {
    match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x86_64",
        Ok("x86") => "i686",
        Ok("aarch64") => "aarch64",
        Ok("arm") => "armv7l",
        _ => "x86_64",
    }
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

    let use_stub = std::env::var("TSI_STUB").is_ok();

    let mut build = cc::Build::new();

    if use_stub {
        build
            .include("vendor/tsi_stub/include")
            .file("src/adapters/tsi/shim.c")
            .file("vendor/tsi_stub/tsi_stub.c")
            .warnings(false)
            .compile("tsi_shim");

        println!("cargo:rerun-if-env-changed=TSI_STUB");
        println!("cargo:rerun-if-changed=src/adapters/tsi/shim.c");
        println!("cargo:rerun-if-changed=vendor/tsi_stub/include/tl_camera_sdk.h");
        println!("cargo:rerun-if-changed=vendor/tsi_stub/tsi_stub.c");
        return;
    }

    let sdk_root = find_tsi_sdk_root();

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
    println!("cargo:rerun-if-env-changed=TSI_STUB");
    println!("cargo:rerun-if-changed=src/adapters/tsi/shim.c");
}

fn find_tsi_sdk_root() -> String {
    if let Ok(root) = std::env::var("TSI_SDK_ROOT") {
        return root;
    }
    if let Ok(root) = std::env::var("THORLABS_TSI_SDK_PATH_64_BIT") {
        return root;
    }
    if let Ok(root) = std::env::var("THORLABS_TSI_SDK_PATH_32_BIT") {
        return root;
    }
    let mut searched: Vec<&'static str> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        for c in &[
            "/Library/Application Support/Thorlabs/Scientific Camera SDK",
            "/usr/local/thorlabs/tsi_sdk",
        ] {
            searched.push(c);
            if std::path::Path::new(c).exists() {
                return c.to_string();
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for c in &["/opt/thorlabs/tsi_sdk", "/usr/local/tsi_sdk"] {
            searched.push(c);
            if std::path::Path::new(c).exists() {
                return c.to_string();
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let c = r"C:\Program Files\Thorlabs\Scientific Imaging\Scientific Camera SDK";
        searched.push(c);
        if std::path::Path::new(c).exists() {
            return c.to_string();
        }
    }
    panic!(
        "Thorlabs Scientific Camera SDK not found. Set TSI_SDK_ROOT \
         (or THORLABS_TSI_SDK_PATH_64_BIT / THORLABS_TSI_SDK_PATH_32_BIT) \
         to a root containing tl_camera_sdk.h and libtl_camera_sdk.*. Searched: {}",
        searched.join(", ")
    );
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

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if pkg_config::Config::new()
        .atleast_version("2.0")
        .probe("libdc1394-2")
        .is_ok()
    {
        return;
    }

    match target_os.as_str() {
        "macos" => {
            println!("cargo:rustc-link-search=/opt/homebrew/lib");
            println!("cargo:rustc-link-search=/usr/local/lib");
            println!("cargo:rustc-link-lib=dc1394");
        }
        "linux" => {
            panic!("libdc1394-2 was not found by pkg-config; install libdc1394-dev and pkg-config");
        }
        _ => {
            println!("cargo:rustc-link-lib=dc1394");
        }
    }
}
