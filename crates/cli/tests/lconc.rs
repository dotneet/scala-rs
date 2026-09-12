//! The concurrency / conversion / interop corner of scala/scala's own standard
//! library (`agent/libconc`).
//!
//! Five independent roots, each reduced to a standalone program and checked
//! against real scalac 2.13.16 in both directions:
//!
//! 1. `_ => _` as a parameter type -- an existential `Function1[_, _]`. The
//!    contravariant parameter position flipped against the wildcard instead of
//!    containing the actual type, so all eleven `new Transformation[T, S](tag,
//!    f, ec)` calls in `scala/concurrent/impl/Promise.scala` were rejected.
//! 2. `def f(...): T = macro ???`. nsc skips the macro-def/macro-impl
//!    correspondence check for the `Predef.???` placeholder, so the definition
//!    stands and only a call site is refused ("macro implementation is
//!    missing"). `StringContext.s/f/raw` and `scala.reflect
//!    .materializeClassTag` are declared this way.
//! 3. Inference through a bounded existential on *both* sides
//!    (`Collection[_ <: Callable[T]]` against `Collection[_ <: Callable[U]]`),
//!    which the four `invokeAll`/`invokeAny` forwarders in
//!    `ExecutionContextImpl.scala` need.
//! 4. A compound whose `apply` comes from a function parent -- `val b:
//!    ManagedBlocker with (() => T)` applied as `b()` -- and a trait whose
//!    declared parent *is* a function type (`trait PartialFunction[-A, +B]
//!    extends (A => B)`, whose `applyOrElse` calls `apply(x)` unqualified).
//! 5. `protected def clone(): AnyRef` on `scala.AnyRef`, which
//!    `collection/mutable/Cloneable.scala` reaches through `super.clone()`.
//!
//! Fixture prefix: `lconc_`.

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
        "scala-rs-lconc-{tag}-{}-{nanos}-{seq}",
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

/// Compile `name` with `compiler` (scala-rs when `scalac_path` is `None`), run
/// it, and compare with `tests/fixtures/expected/<name>.txt`.
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
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &jar), expected, "{name}");
    let _ = fs::remove_dir_all(&dir);
}

/// `name` must be refused, and the diagnostics must mention each `needles`
/// fragment. `scalac_path` checks that real scalac refuses it too.
fn check_rejects(name: &str, needles: &[&str], scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
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
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "{name} was accepted; it must be refused:\n{text}"
    );
    // The wording differs between the two compilers, so only scala-rs's own
    // messages are matched.
    if scalac_path.is_none() {
        for n in needles {
            assert!(text.contains(n), "{name}: expected {n:?} in:\n{text}");
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// 1. `_ => _`, an existential function type, as a parameter type.

#[test]
fn wildcard_function_parameter_runs() {
    check_runs("lconc_wildfun", None);
}

#[test]
fn scalac_agrees_wildcard_function_parameter() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lconc_wildfun", Some(&sc));
}

#[test]
fn wildcard_function_parameter_rejects() {
    check_rejects(
        "lconc_wildfun_bad",
        &["required: (Int) => String", "with arguments (42)"],
        None,
    );
}

#[test]
fn scalac_agrees_wildcard_function_parameter_rejects() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lconc_wildfun_bad", &[], Some(&sc));
}

// ---------------------------------------------------------------------------
// 2. `= macro ???`, the placeholder implementation reference.

#[test]
fn macro_placeholder_definition_runs() {
    check_runs("lconc_macroplaceholder", None);
}

#[test]
fn scalac_agrees_macro_placeholder_definition() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lconc_macroplaceholder", Some(&sc));
}

#[test]
fn macro_placeholder_call_rejects() {
    check_rejects(
        "lconc_macroplaceholder_bad",
        &["macro implementation is missing"],
        None,
    );
}

#[test]
fn scalac_agrees_macro_placeholder_call_rejects() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lconc_macroplaceholder_bad", &[], Some(&sc));
}

// ---------------------------------------------------------------------------
// 3. Inference through a bounded existential on both sides.

#[test]
fn bounded_wildcard_inference_runs() {
    check_runs("lconc_wildbound", None);
}

#[test]
fn scalac_agrees_bounded_wildcard_inference() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lconc_wildbound", Some(&sc));
}

#[test]
fn bounded_wildcard_inference_rejects() {
    check_rejects(
        "lconc_wildbound_bad",
        &[
            "with arguments (Collection[_ <: Other[String]])",
            "with arguments (Collection[_ <: Callable[String]])",
        ],
        None,
    );
}

#[test]
fn scalac_agrees_bounded_wildcard_inference_rejects() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lconc_wildbound_bad", &[], Some(&sc));
}

// ---------------------------------------------------------------------------
// 4. A function parent: inside a compound, and as a trait's declared parent.

#[test]
fn function_parent_and_compound_apply_runs() {
    check_runs("lconc_compoundfun", None);
}

#[test]
fn scalac_agrees_function_parent_and_compound_apply() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lconc_compoundfun", Some(&sc));
}

#[test]
fn compound_apply_rejects() {
    check_rejects(
        "lconc_compoundfun_bad",
        &["value apply is not a member of Marker with Cloneable"],
        None,
    );
}

#[test]
fn scalac_agrees_compound_apply_rejects() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lconc_compoundfun_bad", &[], Some(&sc));
}

// ---------------------------------------------------------------------------
// 5. `protected def clone(): AnyRef` on `scala.AnyRef`.

#[test]
fn anyref_clone_runs() {
    check_runs("lconc_anyrefclone", None);
}

#[test]
fn scalac_agrees_anyref_clone() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lconc_anyrefclone", Some(&sc));
}

#[test]
fn anyref_clone_protected_rejects() {
    check_rejects("lconc_anyrefclone_bad", &["cannot be accessed"], None);
}

#[test]
fn scalac_agrees_anyref_clone_protected_rejects() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lconc_anyrefclone_bad", &[], Some(&sc));
}
