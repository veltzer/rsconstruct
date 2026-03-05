use std::process::Command;

fn git(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn main() {
    let sha = git(&["rev-parse", "HEAD"]);
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let describe = git(&["describe", "--tags", "--always"]);

    let rustc_ver = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).to_string();
            // "rustc 1.93.1 (083ac5135 ...)" -> "1.93.1"
            s.split_whitespace().nth(1).map(|v| v.to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned());

    let edition = std::fs::read_to_string("Cargo.toml")
        .ok()
        .and_then(|s| s.lines()
            .find(|l| l.starts_with("edition"))
            .and_then(|l| l.split('=').nth(1))
            .map(|v| v.trim().trim_matches('"').to_owned()))
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=RSBUILD_RUST_EDITION={edition}");
    println!("cargo:rustc-env=VERGEN_GIT_SHA={sha}");
    println!("cargo:rustc-env=VERGEN_GIT_BRANCH={branch}");
    println!("cargo:rustc-env=VERGEN_RUSTC_SEMVER={rustc_ver}");
    println!("cargo:rustc-env=RSBUILD_GIT_DESCRIBE={describe}");

    // Only re-run when the git HEAD, branch ref, or Cargo.toml changes.
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=.git/HEAD");
    // Read .git/HEAD to find the current ref and watch it.
    // The loose ref file may not exist if git has packed the ref,
    // so fall back to watching .git/packed-refs.
    if let Ok(head) = std::fs::read_to_string(".git/HEAD")
        && let Some(refpath) = head.trim().strip_prefix("ref: ")
    {
        let loose = format!(".git/{refpath}");
        if std::path::Path::new(&loose).exists() {
            println!("cargo:rerun-if-changed={loose}");
        } else {
            println!("cargo:rerun-if-changed=.git/packed-refs");
        }
    }
}
