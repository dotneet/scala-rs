//! E2E tests for the `agent/signature` slice: JVMS §4.7.9 `Signature`
//! attributes, plus the JVMS §4.7.2 `ConstantValue` that `@SerialVersionUID`
//! needs.
//!
//! Before this slice no `Signature` attribute was ever emitted, so every
//! generic member of every class this compiler wrote looked raw to Java —
//! `getGenericInterfaces`, `Method#toGenericString` and `Field#getGenericType`
//! all fell back to the erased shape. `docs/scala-corpus.md` named it the
//! largest remaining root in the corpus's `run` set.
//!
//! The signatures are built in `crates/backend/src/sig.rs`, before the erasure
//! phase rewrites symbol types in place, and attached only when they erase
//! back to the descriptor they sit next to — see that module's header for why
//! a refusal is the right answer whenever the two disagree.
//!
//! `sg_sig.scala`'s expected output was taken from **real scalac 2.13.16**,
//! and both of this compiler's modes reproduce it byte for byte.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-signature-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Private-runtime run (`--no-scala-library`).
#[test]
fn generic_signatures_private_runtime() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with("sg_sig", &["--no-scala-library"]);
    let got = run_java(&out, None, "sg.Main");
    assert_eq!(got, expected_stdout("sg_sig"));
    let _ = fs::remove_dir_all(&out);
}

/// Library-ABI run (`--scala-library <jar>`).
#[test]
fn generic_signatures_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("sg_sig", &["--scala-library", jar_s]);
    let got = run_java(&out, Some(jar_s), "sg.Main");
    assert_eq!(got, expected_stdout("sg_sig"));
    let _ = fs::remove_dir_all(&out);
}

/// The other half of the claim: a member that says nothing beyond its
/// descriptor gets **no** attribute. A `Signature` on every method would pass
/// the reflection tests above just as well and be pure noise in every class
/// file, so this is checked directly against `javap -v`.
#[test]
fn a_monomorphic_member_carries_no_signature() {
    let Ok(javap) = which_javap() else { return };
    let out = compile_fixture_with("sg_sig", &["--no-scala-library"]);
    let text = Command::new(&javap)
        .args(["-v", "-p", "-cp"])
        .arg(&out)
        .arg("sg.C")
        .output()
        .expect("javap");
    let text = String::from_utf8_lossy(&text.stdout).into_owned();
    // `javap` prints `descriptor:` then the member's other attributes, both
    // indented four spaces; the class's own `Signature` sits at column zero.
    // The member header is no good to match on, because javap prints the
    // *generic* form of a member that has a signature.
    let mut last_desc = String::new();
    let mut signed: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(d) = line.strip_prefix("    descriptor: ") {
            last_desc = d.trim().to_string();
        } else if line.starts_with("    Signature:") {
            signed.push(std::mem::take(&mut last_desc));
        }
    }
    assert!(
        !signed.iter().any(|d| d == "(I)I"),
        "e(int) has no generic information and must carry no Signature:\n{text}"
    );
    assert!(
        signed.iter().any(|d| d == "(Lsg/Wrapper;)I"),
        "a(Wrapper) is generic and must carry a Signature:\n{text}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn which_javap() -> Result<PathBuf, ()> {
    let p = PathBuf::from("javap");
    match Command::new(&p).arg("-version").output() {
        Ok(o) if o.status.success() => Ok(p),
        _ => Err(()),
    }
}
