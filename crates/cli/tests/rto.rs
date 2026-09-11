//! Silent miscompilations found through the scala/scala `run` corpus: programs
//! that compiled and ran but printed something else than scalac's output.
//!
//! * `rto_companions`: serializable companions (SD-290), a written or
//!   inherited `apply` replacing the synthetic case one (t10389, t10261),
//!   value-class equality over a universal trait (t6534), inherited
//!   `equals`/`toString` on case classes (proxy), `@transient object`
//!   (transient-object).
//! * `rto_patterns`: singleton and literal type tests (t4577, t12312,
//!   sip23-patterns) and which identifier patterns bind (identifierCase).
//! * `rto_dispatch`: qualifier side effects before a static module (t4859),
//!   a value alternative over eta-expansion (t9395), weak conformance in
//!   overload applicability (t12560), `classOf` constants (classof, t4871),
//!   `ClassTag.AnyVal` (classtags_core), per-class `delayedInit` endpoints
//!   (t6481).
//! * `rto_escapes`: escapes in triple-quoted and interpolated strings
//!   (t3220-213, t8015-ffc).
//!
//! Each fixture is run twice: compiled by scala-rs, and compiled by scalac
//! 2.13.16, both against the same expected output.
//!
//! Fixture prefix: `rto_`.

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
        "scala-rs-rto-{tag}-{}-{nanos}-{seq}",
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
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `name` with scala-rs, or with scalac when `scalac_path` is given,
/// run it, and compare with the expected output.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-nowarn",
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    };
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

fn check_both(name: &str) {
    check_runs(name, None);
    match scalac() {
        Some(sc) => check_runs(name, Some(&sc)),
        None => eprintln!("skip: scalac not present"),
    }
}

#[test]
fn rto_companions_run_like_scalac() {
    check_both("rto_companions");
}

#[test]
fn rto_patterns_run_like_scalac() {
    check_both("rto_patterns");
}

#[test]
fn rto_dispatch_runs_like_scalac() {
    check_both("rto_dispatch");
}

#[test]
fn rto_escapes_run_like_scalac() {
    check_both("rto_escapes");
}

/// `rto_sepapply_1.scala` compiled on its own, then `rto_sepapply_2.scala`
/// against its classfiles, by scala-rs and by scalac.
fn check_separate(scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("sepapply");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    for part in ["rto_sepapply_1", "rto_sepapply_2"] {
        let src = fixtures_dir().join(format!("{part}.scala"));
        let output = match scalac_path {
            Some(sc) => Command::new(sc)
                .args(["-nowarn", "-classpath"])
                .arg(format!("{}:{}", jar.display(), out.display()))
                .args(["-d", out.to_str().unwrap()])
                .arg(&src)
                .output()
                .expect("run scalac"),
            None => Command::new(bin())
                .arg("compile")
                .arg(&src)
                .args(["-d", out.to_str().unwrap(), "-cp", out.to_str().unwrap()])
                .args(["--scala-library", jar.to_str().unwrap()])
                .output()
                .expect("run scala-rs compile"),
        };
        assert!(
            output.status.success(),
            "compile {part} failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join("rto_sepapply.txt")).unwrap();
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rto_inherited_binary_apply_replaces_case_apply() {
    check_separate(None);
    match scalac() {
        Some(sc) => check_separate(Some(&sc)),
        None => eprintln!("skip: scalac not present"),
    }
}

/// A value class may not write its own `equals` or `hashCode`: codegen always
/// gives it the pair nsc's `SyntheticMethods` does, so accepting one would
/// drop it silently.
#[test]
fn rto_value_class_equals_redefinition_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("rto_vceq_bad.scala");
    let dir = tmp_dir("vceq_bad");
    let output = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .args(["-d", dir.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{text}");
    for what in ["equals", "hashCode"] {
        let msg = format!(
            "redefinition of {what} method. See SIP-15, criterion 5. is not allowed in value class"
        );
        assert!(text.contains(&msg), "missing `{msg}` in:\n{text}");
    }
    if let Some(sc) = scalac() {
        let o = Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                dir.to_str().unwrap(),
            ])
            .arg(&src)
            .output()
            .expect("run scalac");
        let s = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(
            !o.status.success() && s.matches("redefinition of").count() == 2,
            "{s}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
