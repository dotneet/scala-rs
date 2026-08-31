//! `case class` declared inside a method body ("local").
//!
//! The synthetic companion module (`apply`, giving `P(1)` a way to build a
//! `P`) was only ever emitted for a *top-level* `case class` by
//! `Backend::walk_stats`. The `Block` arm of `Backend::emit_anon_classes`,
//! which is where a method-local `class`/`object` is actually reached, called
//! `emit_class` for a local `ClassDef` but never `emit_case_companion`. The
//! type checker had already linked a companion symbol
//! (`Typer::ensure_companion`), so `P(1)` type-checked and then blew up at
//! run time with `NoClassDefFoundError: Main$P$1$` -- a silent miscompile,
//! not a compile error.
//!
//! Every fixture is run twice: against the private runtime
//! (`--no-scala-library`) and against the real scala-library jar
//! (`--scala-library`), which must both print what real scalac prints.

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
        "scala-rs-localcc-{tag}-{}-{nanos}-{seq}",
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
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
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
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
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

/// Compile and run `name` with the private runtime and with the jar; both
/// must print `tests/fixtures/expected/<name>.txt` -- captured from real
/// scalac 2.13.16 (`/tmp/scala-2.13.16/bin/scalac`) running the same source.
fn check_both(name: &str) {
    if !java_available() {
        return;
    }
    let exp = expected_stdout(name);

    let out = compile_fixture_with(name, &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        exp,
        "stdout mismatch for {name} (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name} library run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        exp,
        "stdout mismatch for {name} (scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails(name: &str, needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------------- run

/// The reported bug, almost verbatim: `case class P(n: Int)` inside `main`,
/// used both as a constructor (`P(1)`) and as a pattern (`case P(x) => ...`).
#[test]
fn fixtures_lcc1_local_case_class_companion() {
    check_both("lcc1");
}

/// A local `case object` has no separate synthetic companion to lose -- the
/// `object` declaration itself already went through `emit_module`. Pinned
/// down so the case-class companion fix cannot regress the object form.
#[test]
fn fixtures_lcc2_local_case_object() {
    check_both("lcc2");
}

/// Two methods each declaring `case class P(...)`: distinct classes *and*
/// distinct companions, neither leaking into the other method.
#[test]
fn fixtures_lcc3_same_case_class_name_in_two_methods() {
    check_both("lcc3");
}

/// A local `case class` whose body reads an enclosing-method local: real
/// scalac gives the synthetic companion its own capture field and builds a
/// fresh companion instance per call (verified with `javap` against scalac
/// 2.13.16 companion `Cap$Q$2$` in the investigation for this fixture). That
/// shape is the same unimplemented `LazyRef` local-object lowering
/// `check_local_objects` already refuses for a plain local `object`; a local
/// case class hits it through the synthetic companion instead of a written
/// body, so it needs its own check (`check_local_case_class_captures`).
/// Refusing this at compile time turns what was a type-checked
/// `NoSuchMethodError` building `Main$Q$1` at run time into a clean
/// diagnostic.
#[test]
fn fixtures_lcc4_bad_capturing_companion_not_implemented() {
    compile_fails(
        "lcc4_bad",
        "not implemented: a local `case class Q` that reads a local of the enclosing method",
    );
}

// ------------------------------------------------------------------ javap

/// Disassembly of one emitted class.
fn javap(out: &Path, class: &str) -> String {
    let output = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    assert!(
        output.status.success(),
        "javap {class} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The companion classfile has to exist and actually carry `apply` -- the
/// bug was silent otherwise (type-checks, `NoClassDefFoundError` at run
/// time). `Main$P$1` is the class, `Main$P$1$` its companion: our naming
/// reuses the case class's own already-assigned local index for the
/// companion (`Typer::ensure_companion`) rather than drawing a second one
/// the way scalac's `Main$P$1` / `Main$P$2$` does -- both are internal to one
/// compiler's own ABI and not compared bit for bit, only that the two
/// classfiles cross-reference each other correctly and `-Xverify:all`
/// accepts them.
#[test]
fn local_case_class_companion_has_apply() {
    let out = compile_fixture_with("lcc1", &["--no-scala-library"]);
    assert!(
        out.join("Main$P$1.class").is_file(),
        "the case class itself should still be emitted"
    );
    assert!(
        out.join("Main$P$1$.class").is_file(),
        "the synthetic companion module class is missing (the reported bug)"
    );
    let comp = javap(&out, "Main$P$1$");
    assert!(
        comp.contains("public Main$P$1 apply(int)"),
        "companion should declare apply(int): Main$1, got:\n{comp}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Two methods declaring the same local `case class P` must each get their
/// own class *and* their own companion -- four distinct classfiles, none
/// shared between the methods.
#[test]
fn same_named_local_case_classes_get_separate_companions() {
    let out = compile_fixture_with("lcc3", &["--no-scala-library"]);
    let names: Vec<String> = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("Main$P$"))
        .collect();
    for want in ["Main$P$1.class", "Main$P$2.class"] {
        assert!(
            names.contains(&want.to_string()),
            "{want} missing; emitted Main$P$* files: {names:?}"
        );
    }
    let companions: Vec<&String> = names.iter().filter(|n| n.ends_with("$.class")).collect();
    assert_eq!(
        companions.len(),
        2,
        "expected two distinct companions (one per method), got {names:?}"
    );
    let _ = fs::remove_dir_all(&out);
}
