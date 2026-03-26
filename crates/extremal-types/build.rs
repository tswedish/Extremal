use std::process::Command;

fn main() {
    // Capture git commit SHA at build time.
    // Falls back to BUILD_COMMIT env var (set by Docker/CI), then "unknown".
    let commit = git_sha()
        .or_else(|| std::env::var("BUILD_COMMIT").ok())
        .unwrap_or_else(|| "unknown".into());

    println!("cargo:rustc-env=BUILD_COMMIT={commit}");

    // Only re-run if git HEAD changes (not on every source change).
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
    println!("cargo:rerun-if-env-changed=BUILD_COMMIT");
}

fn git_sha() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}
