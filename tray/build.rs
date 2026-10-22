use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("panel-swift").join("Panel.swift");
    println!("cargo:rerun-if-env-changed=TUN4AI_SKIP_PANEL");
    // Only for type-checking the Rust side from a machine without swiftc (e.g. Linux with
    // --target x86_64-apple-darwin). Opt-in, so a real build never ships without the panel.
    if env::var_os("TUN4AI_SKIP_PANEL").is_some() {
        println!("cargo:warning=TUN4AI_SKIP_PANEL is set: the SwiftUI panel was not built");
        return;
    }
    let dest_dir = artifact_dir();
    std::fs::create_dir_all(&dest_dir).expect("create target dir");
    let dest = dest_dir.join("Tun4aiPanel");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let opt = if profile == "release" { "-O" } else { "-Onone" };
    let mut cmd = Command::new("swiftc");
    cmd.arg(opt).arg("-parse-as-library");
    if let Some(target) = swift_target() {
        cmd.arg("-target").arg(target);
    }
    let status = cmd.arg("-o").arg(&dest).arg(&src).status().expect("swiftc");
    if !status.success() {
        panic!("swiftc failed to build the panel");
    }
    println!("cargo:rerun-if-changed={}", src.display());
}

fn artifact_dir() -> PathBuf {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    out.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

fn swift_target() -> Option<String> {
    let target = env::var("TARGET").unwrap_or_default();
    let arch = if target.starts_with("aarch64") {
        "arm64"
    } else if target.starts_with("x86_64") {
        "x86_64"
    } else {
        return None;
    };
    Some(format!("{arch}-apple-macos12.0"))
}
