//! E2E tests for the `agent/verifyfail` slice: the six classes slick compiles
//! to that the JVM refuses to load.
//!
//! `tests/slick_measure.sh` reported `errors=0 classes=1490` while six of
//! those 1490 failed `VerifyError` at link time, and had for an unknown number
//! of waves. Nothing in the battery could see them: `slick_subset.sh` loads
//! every class with `Class.forName(name, false, loader)`, and *not
//! initialising* means not linking, so no method body is ever verified;
//! `slick_run.sh` verifies what its twelve programs touch; `classfile_lint.py`
//! reads structure and types nothing; a `javap -p` sweep stops at the constant
//! pool. `tests/verify_all.sh` is the check that was missing.
//!
//! Six failures, four roots:
//!
//! | class | JVM message | root |
//! | --- | --- | --- |
//! | `PostgresProfile$PostgresQueryBuilder` | `Bad invokespecial instruction` | `super.m` resolved through a stale overload group |
//! | `DistributedProfile` | `Bad return type` | a bridge handed on a `Nothing$` |
//! | `MemoryProfile$InsertMappingCompiler$InsertResultConverter` | `Bad type on operand stack` | a bridge parameter typed `Nothing` |
//! | `MemoryQueryingProfile$MemoryCodeGen$QueryResultConverter` | `Bad type on operand stack` | (same) |
//! | `PositionedResult$$anon$507` | `Bad type on operand stack` | outer read off `uninitializedThis` |
//! | `HList$` | `Bad type on operand stack` | extractor reached through a `val` alias |
//!
//! Each fixture is the smallest program that reproduces one of them; every
//! test here fails on the parent commit. The `super` root also silently turned
//! four *other* profiles' `super.expr(n)` into a call to their own `expr`,
//! which verifies perfectly and recurses for ever -- `vf_super` pins that
//! shape too.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.
//!
//! Fixture prefix: `vf_`.

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
        "scala-rs-verifyfail-{tag}-{}-{nanos}-{seq}",
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
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// `-Xverify:all` is redundant on a modern JVM (the split verifier runs
/// anyway) and harmless; it is spelled out because this whole file is about
/// verification and a future `-Xverify:none` default would make every test
/// here pass for the wrong reason.
fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "vf.Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java vf.Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Private-runtime run (`--no-scala-library`).
fn check(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let got = run_java(&out, None);
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// library-ABI run (`--scala-library <jar>`).
fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    let got = run_java(&out, Some(jar_s));
    assert_eq!(
        got,
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_errors(name: &str, extra: &[&str]) -> String {
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
    let _ = fs::remove_dir_all(&out);
    err
}

/// `javap -c -p` of one class in a freshly compiled fixture.
fn javap(out: &Path, class: &str) -> String {
    let output = Command::new("javap")
        .args(["-c", "-p", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// `super.m` against a stale overload group
// ---------------------------------------------------------------------------

#[test]
fn fixtures_vf_super() {
    check("vf_super");
}

#[test]
fn fixtures_vf_super_lib() {
    dual_run_fixture("vf_super");
}

/// The run above only proves the program behaves; this proves *which* method
/// the `invokespecial` names. A `super.expr` that resolves to the class's own
/// `expr` runs for ever rather than failing verification, so `PostgresQB`'s
/// target class is the thing to assert.
#[test]
fn vf_super_invokespecial_names_the_parent() {
    let out = compile_fixture_with("vf_super", &["--no-scala-library"]);
    for cls in [
        "vf.PostgresP$PostgresQB",
        "vf.OracleP$OracleQB",
        "vf.MysqlP$MysqlQB",
    ] {
        let text = javap(&out, cls);
        let supers: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("invokespecial") && l.contains("expr:"))
            .collect();
        assert!(
            !supers.is_empty(),
            "{cls} has no super call to expr:\n{text}"
        );
        for line in supers {
            assert!(
                line.contains("vf/Comp$QB.expr:"),
                "{cls} super-calls something other than vf/Comp$QB.expr: {line}"
            );
        }
    }
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------------------------------------
// `Nothing` through a bridge
// ---------------------------------------------------------------------------

#[test]
fn fixtures_vf_nothing() {
    check("vf_nothing");
}

#[test]
fn fixtures_vf_nothing_lib() {
    dual_run_fixture("vf_nothing");
}

/// The `update` bridge cannot be reached from Scala source, so the run above
/// only proves the class *links*. Check the shape too, against what `javap -c`
/// shows real scalac 2.13.16 emitting: `checkcast scala/runtime/Nothing$` on
/// the argument, `athrow` after the call.
#[test]
fn vf_nothing_bridges_match_scalac() {
    let out = compile_fixture_with("vf_nothing", &["--no-scala-library"]);
    let c1 = javap(&out, "vf.C1");
    assert!(
        c1.contains("checkcast") && c1.contains("scala/runtime/Nothing$"),
        "C1's update bridge should cast its argument to Nothing$:\n{c1}"
    );
    let bridge = c1
        .split("public void update(java.lang.Object, java.lang.Object)")
        .nth(1)
        .unwrap_or("")
        .split("\n\n")
        .next()
        .unwrap_or("")
        .to_string();
    assert!(
        bridge.contains("athrow") && !bridge.contains("return"),
        "C1's update bridge should end in athrow, not return:\n{bridge}"
    );
    let impl_ = javap(&out, "vf.Impl$");
    let getter = impl_
        .split("public java.lang.String compiler()")
        .nth(1)
        .unwrap_or("")
        .split("\n\n")
        .next()
        .unwrap_or("")
        .to_string();
    assert!(
        getter.contains("athrow") && !getter.contains("areturn"),
        "Impl$'s String-returning bridge should end in athrow:\n{getter}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------------------------------------
// the enclosing instance in a super-constructor argument
// ---------------------------------------------------------------------------

#[test]
fn fixtures_vf_outer() {
    check("vf_outer");
}

#[test]
fn fixtures_vf_outer_lib() {
    dual_run_fixture("vf_outer");
}

/// The pre-super region may not read slot 0 at all. `putfield` on
/// `uninitializedThis` is the one thing JVMS §4.10.1.9 allows, so the only
/// `aload_0` before the `invokespecial <init>` is the receiver of that call
/// and of the `$outer` store.
#[test]
fn vf_outer_presuper_reads_the_parameter() {
    let out = compile_fixture_with("vf_outer", &["--no-scala-library"]);
    let text = javap(&out, "vf.PR$$anon$1");
    let ctor = text
        .split("vf.PR$$anon$1(")
        .nth(1)
        .unwrap_or("")
        .split("\n\n")
        .next()
        .unwrap_or("")
        .to_string();
    let presuper: Vec<&str> = ctor
        .split("invokespecial")
        .next()
        .unwrap_or("")
        .lines()
        .filter(|l| l.contains(':'))
        .collect();
    // Reading `this` is only legal here as the receiver of the `$outer`
    // `putfield`, so no `getfield` / `invokevirtual` may follow an `aload_0`.
    for w in presuper.windows(2) {
        let bad = w[1].contains("getfield") || w[1].contains("invokevirtual");
        assert!(
            !(w[0].contains("aload_0") && bad),
            "the pre-super region reads a member off uninitializedThis:\n{ctor}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------------------------------------
// an object reached through an alias
// ---------------------------------------------------------------------------

#[test]
fn fixtures_vf_alias() {
    check("vf_alias");
}

#[test]
fn fixtures_vf_alias_lib() {
    dual_run_fixture("vf_alias");
}

/// The receiver of `HC$.unapply` is `HC$`, not the object the *name* was found
/// in.
#[test]
fn vf_alias_extractor_receiver_is_the_extractor() {
    let out = compile_fixture_with("vf_alias", &["--no-scala-library"]);
    let text = javap(&out, "vf.Main$");
    let lines: Vec<&str> = text.lines().collect();
    let calls: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("vf/HC$.unapply"))
        .map(|(i, _)| i)
        .collect();
    assert!(
        !calls.is_empty(),
        "expected a call to vf/HC$.unapply:\n{text}"
    );
    for i in calls {
        // The receiver is pushed a few instructions earlier (the scrutinee and
        // its `checkcast` sit between); `vf/syn$` there is the bug.
        let window = &lines[i.saturating_sub(4)..i];
        assert!(
            window.iter().any(|l| l.contains("vf/HC$.MODULE$")),
            "the receiver of HC$.unapply is not HC$:\n{}",
            window.join("\n")
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// A name that answers only in the type namespace is not a value. Real scalac
/// 2.13.16 reports `not found: value OnlyType`; we used to emit
/// `throw new RuntimeException("cannot load OnlyType")` and say nothing.
#[test]
fn fixtures_vf_alias_bad_is_error() {
    let err = compile_errors("vf_alias_bad", &["--no-scala-library"]);
    assert!(
        err.contains("not found: value OnlyType"),
        "expected `not found: value OnlyType`, got: {err}"
    );
    assert!(
        !err.contains("cannot load"),
        "the stub must be gone, not merely reported: {err}"
    );
}
