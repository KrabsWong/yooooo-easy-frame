use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=SPARKLE_FRAMEWORK_DIR");
    println!("cargo:rerun-if-changed=native/app_icon.m");
    println!("cargo:rerun-if-changed=native/updater.m");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("unsupported macOS architecture: {other}"),
    };

    compile_native(&out, arch, "native/app_icon.m", "easy_frame_app_icon", None);
    println!("cargo:rustc-link-lib=framework=AppKit");

    if env::var_os("CARGO_FEATURE_SPARKLE").is_none() {
        return;
    }

    let framework =
        env::var("SPARKLE_FRAMEWORK_DIR").expect("set SPARKLE_FRAMEWORK_DIR for Sparkle builds");
    compile_native(
        &out,
        arch,
        "native/updater.m",
        "easy_frame_updater",
        Some(&framework),
    );
    println!("cargo:rustc-link-search=framework={framework}");
    println!("cargo:rustc-link-lib=framework=Sparkle");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
}

fn compile_native(
    out: &PathBuf,
    arch: &str,
    source: &str,
    library: &str,
    framework: Option<&str>,
) {
    let object = out.join(format!("{library}.o"));
    let mut clang = Command::new("clang");
    clang.args([
        "-fobjc-arc",
        "-arch",
        arch,
        "-mmacosx-version-min=14.0",
        "-c",
    ]);
    if let Some(framework) = framework {
        clang.arg("-F").arg(framework);
    }
    assert!(clang
        .arg(source)
        .arg("-o")
        .arg(&object)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("ar")
        .arg("rcs")
        .arg(out.join(format!("lib{library}.a")))
        .arg(&object)
        .status()
        .unwrap()
        .success());

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static={library}");
}
