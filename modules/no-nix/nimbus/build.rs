//! Bake the Vulkan loader's location into the binary (RUNPATH plus a
//! compile-time hint for `gfx::preload_vulkan_loader`), so `nimbus` works
//! when started without LD_LIBRARY_PATH: from niri binds, swayidle or a
//! systemd unit. The directories come from the build environment
//! (`NIMBUS_VULKAN_LIB_DIRS`, else the LD_LIBRARY_PATH, LIBRARY_PATH and
//! NIX_LDFLAGS `-L` entries that hold libvulkan.so.1: build in a nix-shell
//! with `vulkan-loader` and it is found).
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=NIMBUS_VULKAN_LIB_DIRS");
    println!("cargo:rerun-if-env-changed=LD_LIBRARY_PATH");
    println!("cargo:rerun-if-env-changed=LIBRARY_PATH");
    println!("cargo:rerun-if-env-changed=NIX_LDFLAGS");
    let dirs: Vec<String> = match std::env::var("NIMBUS_VULKAN_LIB_DIRS") {
        Ok(d) => d.split(':').filter(|s| !s.is_empty()).map(String::from).collect(),
        Err(_) => {
            let env = |k: &str| std::env::var(k).unwrap_or_default();
            let mut cand: Vec<String> = vec![];
            for k in ["LD_LIBRARY_PATH", "LIBRARY_PATH"] {
                cand.extend(env(k).split(':').map(String::from));
            }
            cand.extend(env("NIX_LDFLAGS").split_whitespace().filter_map(|f| f.strip_prefix("-L")).map(String::from));
            let mut out: Vec<String> = vec![];
            for d in cand {
                if !d.is_empty() && Path::new(&d).join("libvulkan.so.1").exists() && !out.contains(&d) {
                    out.push(d);
                }
            }
            out
        }
    };
    for d in &dirs {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{d}");
    }
    println!("cargo:rustc-env=NIMBUS_VULKAN_LIB_DIRS={}", dirs.join(":"));

    // The commit, for Settings' Profile page, when built from a checkout.
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    if let Some(c) = git(&["rev-parse", "--short=7", "HEAD"]) {
        println!("cargo:rustc-env=NIMBUS_COMMIT={c}");
    }
    if let Some(d) = git(&["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={d}/logs/HEAD");
    }
}
