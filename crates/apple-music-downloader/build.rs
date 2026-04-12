use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const BUILD_COMMIT_ENV: &str = "WRAPPER_GIT_COMMIT";
const FALLBACK_BUILD_VERSION: &str = "00000000";

fn main() {
    println!("cargo:rerun-if-env-changed={BUILD_COMMIT_ENV}");

    // CI/Docker can pass the exact commit explicitly because build contexts usually omit `.git`.
    let version = if let Some(commit) = env::var(BUILD_COMMIT_ENV)
        .ok()
        .filter(|value| !value.is_empty())
    {
        commit_prefix(&commit, BUILD_COMMIT_ENV)
            .unwrap_or_else(|error| fallback_build_version(format!("{BUILD_COMMIT_ENV}: {error}")))
    } else {
        try_git_commit_prefix()
            .unwrap_or_else(|error| fallback_build_version(format!("git metadata: {error}")))
    };
    println!("cargo:rustc-env=WRAPPER_BUILD_VERSION={version}");
}

fn try_git_commit_prefix() -> Result<String, String> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let head_path = try_git_output(&manifest_dir, &["rev-parse", "--git-path", "HEAD"])?;
    println!("cargo:rerun-if-changed={head_path}");

    let packed_refs_path =
        try_git_output(&manifest_dir, &["rev-parse", "--git-path", "packed-refs"])?;
    println!("cargo:rerun-if-changed={packed_refs_path}");

    if let Ok(symbolic_ref) = try_git_output(&manifest_dir, &["symbolic-ref", "-q", "HEAD"]) {
        let ref_path = try_git_output(
            &manifest_dir,
            &["rev-parse", "--git-path", symbolic_ref.trim()],
        )?;
        println!("cargo:rerun-if-changed={ref_path}");
    }

    // Read the commit at build time so /health can identify the exact daemon revision.
    let commit = try_git_output(&manifest_dir, &["rev-parse", "--verify", "HEAD"])?;
    commit_prefix(&commit, "git rev-parse --verify HEAD")
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

fn commit_prefix(commit: &str, source: &str) -> Result<String, String> {
    let commit = commit.trim();
    if commit.len() < 8 || !commit.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!("invalid commit from {source}: `{commit}`"));
    }
    Ok(commit[..8].to_owned())
}

fn fallback_build_version(reason: String) -> String {
    println!(
        "cargo:warning=falling back to build version {FALLBACK_BUILD_VERSION} because {reason}"
    );
    FALLBACK_BUILD_VERSION.to_owned()
}
