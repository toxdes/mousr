use std::{fs, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    if let Ok(head) = fs::read_to_string(".git/HEAD")
        && let Some(reference) = head.strip_prefix("ref: ")
    {
        println!("cargo:rerun-if-changed=.git/{}", reference.trim());
    }

    let revision = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|revision| revision.trim().to_owned())
        .filter(|revision| !revision.is_empty())
        .or_else(packaged_revision)
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=MOUSR_GIT_SHA={revision}");
}

fn packaged_revision() -> Option<String> {
    let metadata = fs::read_to_string(".cargo_vcs_info.json").ok()?;
    let value = metadata.split_once("\"sha1\"")?.1.split_once(':')?.1;
    let revision = value.split('"').nth(1)?;
    (revision.len() >= 8).then(|| revision[..8].to_owned())
}
