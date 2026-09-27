//! A block's locals go out of scope when it ends, and the backend reuses
//! their slots for what follows. Without that every `val` of every branch
//! kept a slot of its own to the end of the method, each branch target's
//! stack map frame was as long as the method's locals, and a method of
//! nested branches was emitted in time quadratic in its length.
//!
//! The program is compiled by scalac and by scala-rs and run; the output has
//! to agree, and the method of forty sibling blocks has to fit in a handful
//! of slots (it took 124 before, as it does in scalac's output, whose frames
//! ASM computes without paying for their length). A loop whose body reuses a
//! released slot for another type checks that frames forget released slots.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-local-slot-reuse-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn run_java(out: &Path, jar: &Path) -> String {
    let o = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "Main",
        ])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "Main failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// `locals=` of the first method whose signature line contains `method`.
fn max_locals(out: &Path, class: &str, method: &str) -> u32 {
    let o = Command::new("javap")
        .args(["-v", "-cp", &out.to_string_lossy(), class])
        .output()
        .expect("run javap");
    let text = String::from_utf8_lossy(&o.stdout);
    let mut in_method = false;
    for line in text.lines() {
        if line.contains(method) {
            in_method = true;
        }
        if in_method {
            if let Some(rest) = line.split("locals=").nth(1) {
                return rest.split(',').next().unwrap().trim().parse().unwrap();
            }
        }
    }
    panic!("no locals for {method} in {class}:\n{text}");
}

#[test]
fn block_locals_release_their_slots() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    let src = fixtures_dir().join("local_slot_reuse.scala");
    let ours = tmp_dir("ours");
    let o = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .arg("-d")
        .arg(&ours)
        .arg("--scala-library")
        .arg(&jar)
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let expected =
        fs::read_to_string(fixtures_dir().join("expected/local_slot_reuse.txt")).unwrap();
    assert_eq!(run_java(&ours, &jar), expected);
    let locals = max_locals(&ours, "Slots$", "public long many(int)");
    assert!(locals <= 10, "many(int) uses {locals} local slots");
    let _ = fs::remove_dir_all(&ours);
    if let Some(scalac) = scalac() {
        let theirs = tmp_dir("scalac");
        let o = Command::new(scalac)
            .env("JAVA_OPTS", "-Xmx2g -Xss8m")
            .arg("-d")
            .arg(&theirs)
            .arg(&src)
            .output()
            .expect("run scalac");
        assert!(o.status.success(), "scalac rejected the fixture");
        assert_eq!(run_java(&theirs, &jar), expected);
        let _ = fs::remove_dir_all(&theirs);
    }
}
