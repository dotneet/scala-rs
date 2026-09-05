//! E2E tests for the `agent/runwrong` slice: programs that compiled, wrote a
//! well-formed classfile, passed the JVM verifier, ran to completion -- and
//! printed the wrong answer.
//!
//! This is the corpus's `run` `output-mismatch` / failed-`assert` pile. No
//! other check in the battery sees it: `javap`, the loader check and the
//! verifier all pass, because nothing about the classfile is malformed. Only
//! running the program against expected output can tell.
//!
//! Eight roots are covered by one fixture, `rw_wronganswer.scala`, whose
//! expected output is pinned by a dual-run against real scalac 2.13.16:
//!
//! 1. **More than 32 `lazy val`s in a class.** `bitmap$0` is a single `int`
//!    and the 33rd bit was `1 << 32`, which reduces to `1 << 0`: forcing the
//!    first `lazy val` made every later one report itself initialised and
//!    return the field's default. `run/t3038c` printed 1..32 and then zeros.
//! 2. **A `private` member of a trait is not inherited** (SLS 5.2). The
//!    member traversal returned it anyway, so whichever parent it reached
//!    first decided the answer -- and a member that does not exist could win
//!    over one that does (`run/t7475b`).
//! 3. **A case class's `equals` must end with `that.canEqual(this)`.**
//!    Without it a case class equals a subclass that explicitly refuses
//!    (`run/caseClassEquality`).
//! 4. **`@volatile` and `final` on a trait's `val`** have to reach the class
//!    that mixes it in. Dropping `@volatile` is a memory-model change with no
//!    other symptom (`run/t8087`, `run/trait_fields_volatile`,
//!    `run/trait_fields_final`, `run/trait_fields_bytecode`).
//! 5. **`@scala.annotation.varargs`** adds a second, Java-shaped entry point
//!    (`f(String[])` beside `f(Seq)`); without it the method is simply not
//!    callable from Java (`run/t5125`, `run/t5125b`).
//! 6. **An empty repeated argument is `Nil`**, not an empty `ArraySeq`; the
//!    callee prints what it was handed (`run/t5966`).
//! 7. **`'sym` is a `scala.Symbol`**, not its name. Codegen pushed the bare
//!    string, so `println('blubber)` printed `blubber` where scalac prints
//!    `Symbol(blubber)` -- and any real `Symbol` member on it would have been
//!    a `NoSuchMethodError` (`run/t4560`, `run/t4601`).
//! 8. **A stable identifier pattern naming an imported classfile member.**
//!    The name resolves to the `val`'s accessor, a nullary *method*, so the
//!    pattern's type was `Type::Method` and `uncurry`'s `eta_if_method`
//!    eta-expanded the pattern itself into `() => Int`. `gen_pattern` does
//!    not know that shape, so it emitted **no test at all**: `import
//!    Int.MaxValue; 5 match { case MaxValue => … }` took the first case.
//!    This one is not in the corpus list above -- it was found while
//!    minimising `run/blame_eye_triple_eee-double` -- and it is the worst of
//!    the eight, because the compiler silently deletes the comparison.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `rw_` prefix.

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
        "scala-rs-runwrong-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    None
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
        "compile {name} failed extra={extra:?}:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// `-Xverify:all` so a wrong stack map or descriptor shows up as a
/// verification failure and not as a silently different answer.
fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_fails(name: &str, extra: &[&str], needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-bad"));
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
        !output.status.success(),
        "expected compile of {name} (extra={extra:?}) to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {name} error to contain {needle:?}, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// All eight roots at once, against the real `scala-library` jar.
#[test]
fn fixtures_rw_wronganswer() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("rw_wronganswer", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("rw_wronganswer"),
        "stdout mismatch for library dual-run rw_wronganswer"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through **real scalac**, so the expected output is pinned
/// to what Scala 2.13.16 prints and not merely to what we print today. Every
/// line of it was a line we used to get wrong.
#[test]
fn real_scalac_dual_run_rw_wronganswer() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip real-scalac dual-run: jar or scalac not obtainable");
        return;
    };
    let dir = tmp_dir("rw_wronganswer-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("rw_wronganswer.scala"))
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac rejected rw_wronganswer:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run_java(&dir, Some(jar.to_str().unwrap())),
        expected_stdout("rw_wronganswer"),
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The other half of root 2: once a trait's `private` member stops answering
/// member lookups from a subclass, naming it has to be a diagnostic rather
/// than a silent binding to the parent's field. scalac says the same thing.
#[test]
fn fixtures_rw_wronganswer_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    compile_fails(
        "rw_wronganswer_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "not found: value hidden",
    );
}

/// scalac rejects the `_bad` fixture too, with the same message.
#[test]
fn real_scalac_rejects_rw_wronganswer_bad() {
    let Some(scalac) = scalac() else {
        eprintln!("skip: scalac not obtainable");
        return;
    };
    let dir = tmp_dir("rw_wronganswer_bad-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("rw_wronganswer_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(!out.status.success(), "scalac accepted rw_wronganswer_bad");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("not found: value hidden"),
        "unexpected scalac error: {err}"
    );
    let _ = fs::remove_dir_all(&dir);
}
