//! `reify { … }` over `val` and `def` definitions bound *inside* the body
//! itself (the `agent/reifydefs` slice, `docs/macros.md` §7.17 "What
//! remains", item 3).
//!
//! Its own file per the project convention (`libctor.rs`, `tqmacro.rs`,
//! `engine.rs`, `rf_reify.rs`), so appending to a shared file's tail never
//! conflicts with another slice doing the same.
//!
//! Two kinds of check, because they catch different things.
//!
//! * `rd_defs.scala` prints `showRaw` of each reified tree and is compared
//!   with real scalac 2.13.16. A `val`/`def` rebuilt with the wrong
//!   `Modifiers`, or a declared type rebuilt as `mkTypeTree(...)` instead of
//!   the structural shape nsc actually uses for a *value* type (as opposed
//!   to a type *argument* -- see `crate::reify::ReifyRef::StaticClass`'s doc
//!   comment), still compiles and runs; only the printed tree tells them
//!   apart.
//! * `rd_defs_valimpl.scala` + `rd_defs_valuse.scala` really expand through
//!   the JVM bridge in two runs and are compared with the same two files
//!   built by real scalac -- the `val` case is the one that round-trips
//!   *end to end*: the engine's reverse wire-format decoder
//!   (`crates/typer/src/expand.rs`) already understood `ValDef` before this
//!   slice (the `agent/staged` slice, `docs/macros.md` §7.13). `DefDef` is
//!   not among the shapes it accepts yet -- a `def` actually invoked as a
//!   macro (rather than `reify`d and printed) fails with "the expansion
//!   contains a `DefDef`, which scala-rs cannot rebuild yet", a pre-existing
//!   and unrelated gap in that decoder, not in `reify` itself. So a `def`
//!   with parameters is verified by `rd_defs.scala`'s `showRaw` comparison
//!   alone, which needs no macro invocation at all (`reify` on
//!   `scala.reflect.runtime.universe` runs standalone).
//!
//! `rd_defs_bad.scala` is the confession: two shapes real scalac compiles
//! and scala-rs still refuses by name -- both are about the *declared type*
//! of a `val`/`def`, not about the definition or the reference to it.

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
        "scala-rs-reifydefs-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scala_reflect_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

/// Everything these tests need. Returns false (and says so) when the machine
/// cannot run them at all -- the same shape as `engine.rs` / `rf_reify.rs`.
fn prerequisites(tag: &str) -> bool {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("skip {tag}: java / javac not available");
        return false;
    }
    if scala_library_jar().is_none() || scala_reflect_jar().is_none() {
        eprintln!("skip {tag}: scala-library / scala-reflect not obtainable");
        return false;
    }
    true
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
        .unwrap_or_else(|e| panic!("read expected/{name}.txt: {e}"))
}

fn compile(name: &str, out: &Path, extra: &[&Path]) -> std::process::Output {
    let jar = scala_library_jar().expect("scala-library");
    let reflect = scala_reflect_jar().expect("scala-reflect");
    let mut cp = reflect.display().to_string();
    for e in extra {
        cp.push(':');
        cp.push_str(&e.display().to_string());
    }
    Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            &cp,
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile")
}

fn scalac_compile(scalac: &Path, name: &str, out: &Path, extra: &[&Path]) {
    let reflect = scala_reflect_jar().expect("scala-reflect");
    let mut cp = reflect.display().to_string();
    for e in extra {
        cp.push(':');
        cp.push_str(&e.display().to_string());
    }
    let res = Command::new(scalac)
        .args([
            "-cp",
            &cp,
            "-d",
            out.to_str().unwrap(),
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        res.status.success(),
        "real scalac rejected {name}.scala: {}",
        String::from_utf8_lossy(&res.stderr)
    );
}

/// Run `Main` and return its stdout, asserting it exited cleanly.
fn run_main(cp: &str, what: &str) -> String {
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for {what}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn classpath(dirs: &[&Path]) -> String {
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let mut cp = String::new();
    for d in dirs {
        cp.push_str(&d.display().to_string());
        cp.push(':');
    }
    cp.push_str(&reflect.display().to_string());
    cp.push(':');
    cp.push_str(&jar.display().to_string());
    cp
}

/// `showRaw` of six reified trees: an untyped `val`, a typed `val`, a `def`
/// with a typed parameter, a recursive `def`, two mutually recursive `def`s,
/// and a `val` read by a `def`. Against the runtime universe.
#[test]
fn rd_defs_reify_and_run() {
    if !prerequisites("rd_defs") {
        return;
    }
    let out_dir = tmp_dir("rd_defs");
    let out = compile("rd_defs", &out_dir, &[]);
    assert!(
        out.status.success(),
        "compile rd_defs failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&classpath(&[&out_dir]), "rd_defs"),
        expected_stdout("rd_defs"),
        "stdout mismatch for rd_defs"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// The same file through real scalac 2.13.16 -- what makes the recorded
/// trees mean something: they are nsc's own, not scala-rs's invention.
#[test]
fn rd_defs_match_real_scalac() {
    if !prerequisites("rd_defs scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rd_defs scalac diff: scalac not obtainable");
        return;
    };
    let out_dir = tmp_dir("rd_defs-scalac");
    scalac_compile(&scalac, "rd_defs", &out_dir, &[]);
    assert_eq!(
        run_main(&classpath(&[&out_dir]), "rd_defs (real scalac build)"),
        expected_stdout("rd_defs"),
        "recorded expectation for rd_defs does not match real scalac"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// The macro path for the one case that round-trips end to end today: a
/// `val` bound inside `reify { … }`, expanded by scala-rs and actually run.
#[test]
fn rd_defs_val_expands_and_runs() {
    if !prerequisites("rd_defs_valuse") {
        return;
    }
    let impls = tmp_dir("rd_defs_valimpl");
    let uses = tmp_dir("rd_defs_valuse");
    let out = compile("rd_defs_valimpl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile rd_defs_valimpl failed: {}",
        diagnostics(&out)
    );
    let out = compile("rd_defs_valuse", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile rd_defs_valuse failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&classpath(&[&uses, &impls]), "rd_defs_valuse"),
        expected_stdout("rd_defs_valuse"),
        "stdout mismatch for rd_defs_valuse"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The same two files through real scalac 2.13.16.
#[test]
fn rd_defs_val_matches_real_scalac() {
    if !prerequisites("rd_defs_valuse scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rd_defs_valuse scalac diff: scalac not obtainable");
        return;
    };
    let impls = tmp_dir("rd_defs_valimpl-scalac");
    let uses = tmp_dir("rd_defs_valuse-scalac");
    scalac_compile(&scalac, "rd_defs_valimpl", &impls, &[]);
    scalac_compile(&scalac, "rd_defs_valuse", &uses, &[&impls]);
    assert_eq!(
        run_main(
            &classpath(&[&uses, &impls]),
            "rd_defs_valuse (real scalac build)"
        ),
        expected_stdout("rd_defs_valuse"),
        "recorded expectation for rd_defs_valuse does not match real scalac"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The two shapes still refused, each named. Real scalac compiles both; both
/// are about the *declared type* of a `val`/`def`, not about the definition
/// or a reference to it -- see `rd_defs_bad.scala`'s own comments.
#[test]
fn rd_defs_gaps_are_named() {
    if !prerequisites("rd_defs_bad") {
        return;
    }
    let out_dir = tmp_dir("rd_defs_bad");
    let out = compile("rd_defs_bad", &out_dir, &[]);
    assert!(
        !out.status.success(),
        "rd_defs_bad.scala should not compile"
    );
    let text = diagnostics(&out);
    for want in [
        // `List[Int]`: a type constructor applied to arguments.
        "a type argument cannot be rebuilt: `List`, a type constructor applied to type arguments",
        // a locally declared `def`'s own type parameter used in value position
        "a type argument cannot be rebuilt: `U`",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    assert!(
        text.contains("cannot expand reify { ... }"),
        "the report should name reify:\n{text}"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// Real scalac accepts `rd_defs_bad.scala`. Without this the fixture could
/// drift into a program that is simply wrong, and the refusals above would
/// stop being a confession and start looking like correct rejections.
#[test]
fn rd_defs_gaps_are_accepted_by_real_scalac() {
    if !prerequisites("rd_defs_bad scalac") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rd_defs_bad scalac: scalac not obtainable");
        return;
    };
    let out_dir = tmp_dir("rd_defs_bad-scalac");
    scalac_compile(&scalac, "rd_defs_bad", &out_dir, &[]);
    let _ = fs::remove_dir_all(&out_dir);
}
