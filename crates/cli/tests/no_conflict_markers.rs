//! A merge that leaves conflict markers behind still compiles when the markers
//! land in prose, and README.md carried a set for two commits before anyone
//! noticed. This fails the suite instead.

use std::path::Path;

#[test]
fn no_tracked_file_has_conflict_markers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git ls-files");
    let mut bad = Vec::new();
    for name in out.stdout.split(|b| *b == 0) {
        if name.is_empty() {
            continue;
        }
        let rel = String::from_utf8_lossy(name).into_owned();
        // This file names the markers to look for.
        if rel.ends_with("no_conflict_markers.rs") {
            continue;
        }
        let path = root.join(&rel);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let marked =
                line.starts_with("<<<<<<< ") || line.starts_with(">>>>>>> ") || line == "=======";
            if marked {
                bad.push(format!("{rel}:{}", i + 1));
                break;
            }
        }
    }
    assert!(bad.is_empty(), "conflict markers left in: {bad:?}");
}
