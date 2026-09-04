//! One literal, one classfile — and a receiver evaluated once.
//!
//! slick's 184 sources came out as 2127 classfiles against nsc's 1498, with
//! 716 `$anonfun` closures against nsc's 137. The literals were not typed
//! wrongly: slick has 130 distinct `{ case … }` literals. They were **emitted
//! many times**, by two multipliers that compound:
//!
//! * a `PartialFunction` closure class generated its case bodies into `apply`
//!   and again into `applyOrElse`, so a literal nested inside a case body came
//!   out 2^depth times (one slick literal appeared 128 times);
//! * a `name$default$n` getter took the whole preceding parameter prefix, so a
//!   call omitting k defaults spliced its arguments 2^k times, and its
//!   **receiver** once per getter — which also *evaluated* the receiver that
//!   many times.
//!
//! The fixture pins the run-time behaviour in both ABIs and against real
//! scalac 2.13.16, and the shape (how many closure classfiles the literals
//! produce) so the numbers move deliberately.

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
        "scala-rs-fewerclasses-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
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
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    out
}

fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn class_names(out: &Path) -> Vec<String> {
    let mut v = Vec::new();
    let mut stack = vec![out.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).expect("read output dir") {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().map(|x| x == "class").unwrap_or(false) {
                v.push(p.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    v.sort();
    v
}

/// The whole fixture runs identically on the private runtime and on the real
/// scala-library. `MatchError` out of a `PartialFunction`'s `apply` is part of
/// it: `apply` no longer carries the bodies, it delegates to `applyOrElse`
/// with a `null` fallback, and a null fallback has to throw.
#[test]
fn fixtures_fewerclasses1_runs_in_both_abis() {
    if !java_available() {
        return;
    }
    let exp = expected_stdout("fewerclasses1");

    let out = compile_fixture_with("fewerclasses1", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        exp,
        "stdout mismatch (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip library run: jar not present");
        return;
    };
    let out = compile_fixture_with("fewerclasses1", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        exp,
        "stdout mismatch (scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `mk().infer()` prints `receivers=1`, not `receivers=3`. The recorded
/// expectation is real scalac 2.13.16's own stdout.
#[test]
fn fewerclasses1_matches_real_scalac() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff: toolchain not obtainable");
        return;
    };
    let src = fixtures_dir().join("fewerclasses1.scala");
    let ref_out = tmp_dir("scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(
        status.success(),
        "real scalac failed to compile the fixture"
    );
    let ref_cp = format!("{}:{}", ref_out.display(), jar.display());
    let reference = Command::new("java")
        .args(["-cp", &ref_cp, "Main"])
        .output()
        .expect("java (real scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let reference = String::from_utf8_lossy(&reference.stdout).to_string();
    assert_eq!(
        reference,
        expected_stdout("fewerclasses1"),
        "recorded expectation does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// Shape. The fixture writes five closure literals that still need a class:
/// the outer `PartialFunction`, the one nested in its first case body, and the
/// three `{ case … }` arguments of `replace`. Real scalac emits five too. If
/// this number goes up, something is being emitted more than once again.
#[test]
fn fewerclasses1_emits_one_class_per_literal() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip shape check: jar not present");
        return;
    };
    let out = compile_fixture_with("fewerclasses1", &["--scala-library", jar.to_str().unwrap()]);
    let names = class_names(&out);
    let closures = names.iter().filter(|n| n.contains("anonfun")).count();
    assert_eq!(
        closures, 5,
        "one classfile per `{{ case … }}` literal, got {names:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A `name$default$n` getter takes the *preceding parameter clauses*, the way
/// nsc's does — not every parameter that happens to come before it. The
/// exception is the same-clause reference nsc rejects and scala-rs accepts:
/// there the parameter is kept, because the body reads it.
#[test]
fn default_getters_take_only_preceding_clauses() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip descriptor check: jar not present");
        return;
    };
    let out = compile_fixture_with("fewerclasses1", &["--scala-library", jar.to_str().unwrap()]);
    let ops = fs::read(out.join("Ops.class")).expect("read Ops.class");
    let ops: String = ops
        .iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' })
        .collect();
    // `replace(f, keepType = false, bottomUp = false)`: one clause, so both
    // getters are nullary.
    assert!(
        ops.contains("replace$default$2") && ops.contains("replace$default$3"),
        "Ops should declare both getters"
    );
    assert!(
        !ops.contains("(Lscala/PartialFunction;)Z"),
        "a getter for a later parameter of the same clause must be nullary, got {ops}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The capability that survived the change. nsc rejects a default that names
/// an earlier parameter of its own clause ("not found: value x", verified
/// against 2.13.16); scala-rs accepts it, so that getter still takes the
/// parameter and the value it computes is still right.
#[test]
fn a_default_reading_an_earlier_parameter_keeps_it() {
    if !java_available() {
        return;
    }
    let dir = tmp_dir("same-clause");
    let src = dir.join("SameClauseRead.scala");
    fs::write(
        &src,
        "object Main {\n  \
           def f(x: Int, y: Int = x + 5, z: Int = 9): Int = x + y + z\n  \
           def main(args: Array[String]): Unit = {\n    \
             println(f(1)); println(f(1, 2)); println(f(1, 2, 3))\n  \
           }\n\
         }\n",
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(
        status.success(),
        "compile of the same-clause default failed"
    );
    let main = fs::read(out.join("Main$.class")).expect("read Main$.class");
    let text: String = main
        .iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' })
        .collect();
    assert!(
        text.contains("f$default$2") && text.contains("(I)I"),
        "the getter that reads `x` must still take it, got {text}"
    );
    assert_eq!(run_java(&out, None), "16\n12\n6\n");
    let _ = fs::remove_dir_all(&dir);
}
