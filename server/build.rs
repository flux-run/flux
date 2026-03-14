//! Build script for the `server` crate.
//!
//! Runs `npm run build` inside `../dashboard` so that the compiled Next.js
//! static export (`dashboard/out/`) is always up-to-date before the Rust
//! binary finishes linking.
//!
//! Cargo re-runs this script only when dashboard source files actually change,
//! so incremental Rust builds are not affected when nothing in the dashboard
//! has changed.

use std::path::Path;
use std::process::Command;

fn main() {
    let dashboard = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dashboard");

    // Tell Cargo to re-run this script only when dashboard sources change.
    // Pointing at a directory causes Cargo to watch it recursively.
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

    // Install node_modules if they are missing (first clone / fresh CI).
    let node_modules = dashboard.join("node_modules");
    if !node_modules.exists() {
        let install = Command::new("npm")
            .args(["install", "--prefer-offline"])
            .current_dir(&dashboard)
            .status()
            .expect("build.rs: failed to spawn `npm install` for dashboard");

        if !install.success() {
            panic!("build.rs: `npm install` failed in dashboard/");
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
}
