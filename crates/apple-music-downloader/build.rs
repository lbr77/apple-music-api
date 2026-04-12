use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let head_path = git_output(&manifest_dir, &["rev-parse", "--git-path", "HEAD"]);
    println!("cargo:rerun-if-changed={head_path}");

    let packed_refs_path = git_output(&manifest_dir, &["rev-parse", "--git-path", "packed-refs"]);
    println!("cargo:rerun-if-changed={packed_refs_path}");

    if let Ok(symbolic_ref) = try_git_output(&manifest_dir, &["symbolic-ref", "-q", "HEAD"]) {
        let ref_path = git_output(
            &manifest_dir,
            &["rev-parse", "--git-path", symbolic_ref.trim()],
        );
        println!("cargo:rerun-if-changed={ref_path}");
    }

    // Read the commit at build time so /health can identify the exact daemon revision.
    let version = git_commit_prefix(&manifest_dir, &["rev-parse", "--verify", "HEAD"]);
    println!("cargo:rustc-env=WRAPPER_BUILD_VERSION={version}");
}

fn git_output(manifest_dir: &Path, args: &[&str]) -> String {
    try_git_output(manifest_dir, args).unwrap_or_else(|error| {
        panic!(
            "failed to resolve build-time git metadata with `git {}`: {error}",
            args.join(" ")
        )
    })
}

fn try_git_output(manifest_dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(manifest_dir)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_commit_prefix(manifest_dir: &Path, args: &[&str]) -> String {
    let commit = git_output(manifest_dir, args);
    commit.chars().take(8).collect()
}
