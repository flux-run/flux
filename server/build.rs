//! Build script for the `server` crate.
//!
//! Runs `npm run build` inside `../dashboard` so that the compiled Next.js
//! static export (`dashboard/out/`) is always up-to-date before the Rust
//! binary finishes linking.
//!
//! When `npm` is not available on PATH (e.g. cross-compilation containers,
//! CI environments that pre-build the dashboard separately) the script checks
//! that `dashboard/out/` already exists and skips npm gracefully.
//!
//! Cargo re-runs this script only when dashboard source files actually change,
//! so incremental Rust builds are not affected when nothing in the dashboard
//! has changed.

use std::path::Path;
use std::process::Command;

fn main() {
    let dashboard = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dashboard");

    // Tell Cargo to re-run this script only when dashboard sources change.
    println!(
        "cargo:rerun-if-changed={}",
        dashboard.join("src").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        dashboard.join("package.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        dashboard.join("next.config.ts").display()
    );

    let out_dir = dashboard.join("out");

    // Check if npm is present (it won't be in cross Docker containers or
    // minimal CI environments that pre-build the dashboard before cargo runs).
    let npm_available = Command::new("npm")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if npm_available {
        // Install node_modules if they are missing (first clone / fresh CI).
        let node_modules = dashboard.join("node_modules");
        if !node_modules.exists() {
            let install = Command::new("npm")
                .args(["ci", "--prefer-offline"])
                .current_dir(&dashboard)
                .status()
                .expect("build.rs: failed to spawn `npm ci` for dashboard");

            if !install.success() {
                panic!("build.rs: `npm ci` failed in dashboard/");
            }
        }

        // Build the Next.js static export → dashboard/out/
        let build = Command::new("npm")
            .args(["run", "build"])
            .current_dir(&dashboard)
            .status()
            .expect("build.rs: failed to spawn `npm run build` for dashboard");

        if !build.success() {
            panic!("build.rs: `npm run build` failed — fix dashboard errors before building the server binary");
        }
    } else if out_dir.exists() {
        // npm is not available but pre-built assets are present (e.g. cross
        // container, or CI where dashboard was built in a dedicated step).
        // Nothing to do — the assets will be embedded as-is.
    } else {
        panic!(
            "build.rs: npm is not on PATH and dashboard/out/ does not exist.\n\
             Either install Node.js or pre-build the dashboard first:\n\
             \n  cd dashboard && npm ci && npm run build\n"
        );
    }
}

