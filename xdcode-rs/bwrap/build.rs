use std::env;
use std::path::Path;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(bwrap_available)");
    println!("cargo:rerun-if-env-changed=XDCODE_BWRAP_SOURCE_DIR");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_ALLOW_CROSS");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_SYSROOT_DIR");
    println!("cargo:rerun-if-env-changed=XDCODE_SKIP_BWRAP_BUILD");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "linux" || env::var_os("XDCODE_SKIP_BWRAP_BUILD").is_some() {
        return;
    }

    if let Err(err) = try_build_bwrap() {
        panic!("failed to compile bubblewrap for Linux target: {err}");
    }
}

fn try_build_bwrap() -> Result<(), String> {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|err| err.to_string())?);
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|err| err.to_string())?);
    let src_dir = resolve_bwrap_source_dir(&manifest_dir)?;
    // Cargo recursively watches directories. Track the resolved tree, including
    // every header and newly added input, for both vendored and override builds.
    println!("cargo:rerun-if-changed={}", src_dir.display());
    let libcap = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("libcap")
        .map_err(|err| format!("libcap not available via pkg-config: {err}"))?;

    let config_h = out_dir.join("config.h");
    std::fs::write(
        &config_h,
        r#"#pragma once
#define PACKAGE_STRING "bubblewrap built for Xdcode"
"#,
    )
    .map_err(|err| format!("failed to write {}: {err}", config_h.display()))?;

    let mut build = cc::Build::new();
    build
        .file(src_dir.join("bubblewrap.c"))
        .file(src_dir.join("bind-mount.c"))
        .file(src_dir.join("network.c"))
        .file(src_dir.join("utils.c"))
        .include(&out_dir)
        .include(&src_dir)
        .define("_GNU_SOURCE", None)
        // Rename `main` so the Rust wrapper can expose the Cargo-built binary.
        .define("main", Some("bwrap_main"));
    for include_path in libcap.include_paths {
        // Use -idirafter so target sysroot headers win (musl cross builds),
        // while still allowing libcap headers from the host toolchain.
        build.flag(format!("-idirafter{}", include_path.display()));
    }

    build.compile("standalone_bwrap");
    for link_path in libcap.link_paths {
        println!("cargo:rustc-link-search=native={}", link_path.display());
    }
    for lib in libcap.libs {
        println!("cargo:rustc-link-lib={lib}");
    }
    println!("cargo:rustc-cfg=bwrap_available");
    Ok(())
}

/// Resolve the bubblewrap source directory used for build-time compilation.
///
/// Priority:
/// 1. `XDCODE_BWRAP_SOURCE_DIR` points at an existing bubblewrap checkout.
/// 2. The vendored bubblewrap tree under `xdcode-rs/vendor/bubblewrap`.
fn resolve_bwrap_source_dir(manifest_dir: &Path) -> Result<PathBuf, String> {
    if let Ok(path) = env::var("XDCODE_BWRAP_SOURCE_DIR") {
        let src_dir = PathBuf::from(path);
        if src_dir.is_dir() {
            return src_dir.canonicalize().map_err(|err| err.to_string());
        }
        return Err(format!(
            "XDCODE_BWRAP_SOURCE_DIR was set but does not exist: {}",
            src_dir.display()
        ));
    }

    let vendor_dir = manifest_dir.join("../vendor/bubblewrap");
    if vendor_dir.is_dir() {
        return vendor_dir.canonicalize().map_err(|err| err.to_string());
    }

    Err(format!(
        "expected vendored bubblewrap at {}, but it was not found.\n\
Set XDCODE_BWRAP_SOURCE_DIR to an existing checkout or vendor bubblewrap under xdcode-rs/vendor.",
        vendor_dir.display()
    ))
}
