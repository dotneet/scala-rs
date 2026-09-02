//! Def-macro *expansion* through the JVM bridge. `docs/macros.md` §2.2, §7.11.
//!
//! Phase 1 could parse `= macro Impl.method` and record the binding; every
//! call site was an error. This is the phase that runs the implementation for
//! real: `crates/typer/src/expand.rs` starts the Java engine
//! (`crates/typer/java/ScalaRsMacroEngine.java`), hands it the argument trees
//! and type tags, and typechecks the tree that comes back at the call site.
//!
//! Two compilations, because nsc requires two: a macro implementation has to
//! come from an *earlier* run, since expanding it means loading its class file.
//! `eg_impl.scala` is compiled first, `eg_use.scala` second with the first
//! one's output on the classpath.
//!
//! The check that matters is the **dual run**: real scalac 2.13.16 compiles
//! the same two files against each other, and the two programs must print the
//! same thing. A macro that expanded to something else would still compile and
//! still run -- only the output would differ -- so comparing output is the
//! only test that can catch a wrong expansion.

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
        "scala-rs-engine-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn javac_available() -> bool {
    Command::new("javac")
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

/// Compile `<name>.scala` with scala-rs, against scala-reflect.jar plus `extra`.
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

/// Everything the engine needs. Returns false (and says so) when the machine
/// cannot run the test at all.
fn prerequisites(tag: &str) -> bool {
    if !java_available() || !javac_available() {
        eprintln!("skip {tag}: java / javac not available");
        return false;
    }
    if scala_library_jar().is_none() || scala_reflect_jar().is_none() {
        eprintln!("skip {tag}: scala-library / scala-reflect not obtainable");
        return false;
    }
    true
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

/// The whole bridge: scala-rs compiles the implementations, then compiles the
/// call sites against them, expanding each macro by really running it.
#[test]
fn eg_macros_expand_and_run() {
    if !prerequisites("eg_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("eg_impl");
    let uses = tmp_dir("eg_use");

    let out = compile("eg_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile eg_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile("eg_use", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile eg_use failed: {}",
        diagnostics(&out)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "eg_use"),
        expected_stdout("eg_use"),
        "stdout mismatch for eg_use"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The same two files through real scalac 2.13.16.
///
/// This is what makes the recorded expectation mean something: it is what nsc
/// produces from the same macro implementations, not what scala-rs happens to
/// build.
#[test]
fn eg_macros_match_real_scalac() {
    if !prerequisites("eg_use scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip eg_use scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("eg_impl-scalac");
    let uses = tmp_dir("eg_use-scalac");

    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            impls.to_str().unwrap(),
            fixtures_dir().join("eg_impl.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected eg_impl.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", reflect.display(), impls.display()),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("eg_use.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected eg_use.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "eg_use (real scalac build)"),
        expected_stdout("eg_use"),
        "recorded expectation for eg_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// A macro whose implementation is defined in the *same* run cannot be
/// expanded -- nsc says the same -- and must still be an error, naming the
/// reason.
#[test]
fn eg_same_run_implementation_is_diagnosed() {
    if !prerequisites("eg_samerun_bad") {
        return;
    }
    let out_dir = tmp_dir("eg_samerun_bad");
    let out = compile("eg_samerun_bad", &out_dir, &[]);
    let err = diagnostics(&out);
    assert!(
        !out.status.success(),
        "expected eg_samerun_bad to fail, got: {err}"
    );
    assert!(
        err.contains("macro expansion is not implemented"),
        "expected the macro diagnostic, got {err:?}"
    );
    assert!(
        err.contains("is not on the macro classpath"),
        "expected the reason to name the missing class, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// Forms the bridge deliberately does not carry are named, one by one. A macro
/// is never quietly accepted: the macro def has no bytecode at all, so a
/// silent pass would emit a call to a method that is not there.
#[test]
fn eg_unsupported_forms_are_named() {
    if !prerequisites("eg_gaps_bad") {
        return;
    }
    let impls = tmp_dir("eg_impl-gaps");
    let out = compile("eg_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile eg_impl failed: {}",
        diagnostics(&out)
    );
    let out_dir = tmp_dir("eg_gaps_bad");
    let out = compile("eg_gaps_bad", &out_dir, &[&impls]);
    let err = diagnostics(&out);
    assert!(
        !out.status.success(),
        "expected eg_gaps_bad to fail, got: {err}"
    );
    for needle in [
        // An argument shape the bridge cannot hand over.
        "cannot hand a block to a macro implementation",
        "cannot hand a function literal to a macro implementation",
        // A type argument no `staticClass` call can rebuild.
        "cannot build a type tag for",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err:?}"
        );
    }
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&out_dir);
}

// ---------------------------------------------------------------------------
// `c.Expr[T](tree)`, `c.prefix`, and a tag composed out of a type constructor
// and the tags in scope. `docs/macros.md` §7.12.
//
// `eg_*` above had to write every implementation body as a bare `c.Tree`:
// `c.Expr[T](tree)` did not resolve to `Context.Expr` at all -- it landed on
// `universe.Expr.apply`, whose parameters are `(Mirror, TreeCreator)` -- and
// `c.prefix` was not implemented. Those are the two members slick's macros
// are written with, so `ex_*` is the same two-stage, dual-run shape for them.

/// The macro implementations of `ex_impl.scala`, expanded for real.
///
/// Three things at once, and the recorded output is real scalac's:
/// implementations that *return* `c.Expr[T]`, implementations that read
/// `c.prefix`, and `c.Expr[ExBox[E]]` -- whose implicit `WeakTypeTag` no
/// program defines and which is composed from `appliedType` over the tag in
/// scope for `E`. That last one is the shape of `TableQueryMacroImpl.apply`.
#[test]
fn ex_expr_and_prefix_macros_expand_and_run() {
    if !prerequisites("ex_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("ex_impl");
    let uses = tmp_dir("ex_use");

    let out = compile("ex_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile ex_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile("ex_use", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile ex_use failed: {}",
        diagnostics(&out)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "ex_use"),
        expected_stdout("ex_use"),
        "stdout mismatch for ex_use"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The same two files through real scalac 2.13.16.
///
/// Without this the recorded expectation would only say what scala-rs does.
/// It is what pins the *composed tag*: `ExBox[ExRow]` is printed out of
/// `weakTypeOf[ExBox[E]]`, and `Nothing` out of `c.prefix.staticType`, which
/// is nsc's answer because nsc builds the prefix as `Expr[Nothing]`.
#[test]
fn ex_macros_match_real_scalac() {
    if !prerequisites("ex_use scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip ex_use scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("ex_impl-scalac");
    let uses = tmp_dir("ex_use-scalac");

    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            impls.to_str().unwrap(),
            fixtures_dir().join("ex_impl.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected ex_impl.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", reflect.display(), impls.display()),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("ex_use.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected ex_use.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "ex_use (real scalac build)"),
        expected_stdout("ex_use"),
        "recorded expectation for ex_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// A tag that cannot be composed is named, not approximated.
///
/// `c.Expr[ExnBox[E]]` needs a `WeakTypeTag[ExnBox[E]]`, and `E` has no tag in
/// scope. nsc goes further and builds one with a free type symbol; scala-rs
/// does not, and the diagnostic says which part is missing. Real scalac
/// compiles this file, so it records a gap rather than an error in the source.
#[test]
fn ex_uncomposable_tag_is_named() {
    if !prerequisites("ex_notag_bad") {
        return;
    }
    let out_dir = tmp_dir("ex_notag_bad");
    let out = compile("ex_notag_bad", &out_dir, &[]);
    let err = diagnostics(&out);
    assert!(
        !out.status.success(),
        "expected ex_notag_bad to fail, got: {err}"
    );
    assert!(
        err.contains("cannot build a WeakTypeTag for `E`, an abstract type with no tag in scope"),
        "expected the missing-tag reason, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// The two receivers `c.prefix` cannot be built from, each named.
#[test]
fn ex_unsupported_prefixes_are_named() {
    if !prerequisites("ex_gaps_bad") {
        return;
    }
    let impls = tmp_dir("ex_impl-gaps");
    let out = compile("ex_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile ex_impl failed: {}",
        diagnostics(&out)
    );
    let out_dir = tmp_dir("ex_gaps_bad");
    let out = compile("ex_gaps_bad", &out_dir, &[&impls]);
    let err = diagnostics(&out);
    assert!(
        !out.status.success(),
        "expected ex_gaps_bad to fail, got: {err}"
    );
    for needle in [
        // No receiver written at all.
        "the macro was called without a receiver",
        // A receiver the bridge will not re-evaluate at the call site.
        "cannot hand a `new` to a macro implementation",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err:?}"
        );
    }
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&out_dir);
}

/// Stage D-1: an expansion containing `Function` and `ValDef`.
///
/// This is the tree slick's `TableQueryMacroImpl.apply` builds --
/// `Function(List(ValDef(Modifiers(Flag.PARAM), TermName("tag"),
/// Ident(typeOf[Tag].typeSymbol), EmptyTree)), Apply(Select(New(TypeTree(
/// e.tpe)), termNames.CONSTRUCTOR), List(Ident(TermName("tag")))))` -- so
/// every node in it has to survive the trip: the function literal, the
/// parameter's modifiers, the type `Ident` built from a symbol, and a
/// reference from the body back to a parameter whose symbol has no name the
/// bridge could carry.
#[test]
fn sd_function_and_valdef_expand_and_run() {
    if !prerequisites("sd_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("sd_impl");
    let uses = tmp_dir("sd_use");

    let out = compile("sd_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile sd_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile("sd_use", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile sd_use failed: {}",
        diagnostics(&out)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "sd_use"),
        expected_stdout("sd_use"),
        "stdout mismatch for sd_use"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The same two files through real scalac 2.13.16.
///
/// A `Function` rebuilt with the wrong parameter name, or a `ValDef` whose
/// modifiers were dropped, would still compile and still run: only the output
/// would differ. This is what makes the recorded expectation nsc's answer
/// rather than scala-rs's.
#[test]
fn sd_function_and_valdef_match_real_scalac() {
    if !prerequisites("sd_use scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip sd_use scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("sd_impl-scalac");
    let uses = tmp_dir("sd_use-scalac");

    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            impls.to_str().unwrap(),
            fixtures_dir().join("sd_impl.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected sd_impl.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", reflect.display(), impls.display()),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("sd_use.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected sd_use.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "sd_use (real scalac build)"),
        expected_stdout("sd_use"),
        "recorded expectation for sd_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The three stage-D forms that are refused, each by name.
#[test]
fn sd_unsupported_forms_are_named() {
    if !prerequisites("sd_gaps_bad") {
        return;
    }
    let impls = tmp_dir("sd_impl-gaps");
    let out = compile("sd_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile sd_impl failed: {}",
        diagnostics(&out)
    );
    let out_dir = tmp_dir("sd_gaps_bad");
    let out = compile("sd_gaps_bad", &out_dir, &[&impls]);
    let err = diagnostics(&out);
    assert!(
        !out.status.success(),
        "expected sd_gaps_bad to fail, got: {err}"
    );
    for needle in [
        // A row class this run compiles is not on the macro classpath yet.
        "class SdLocalRow not found",
        // A nullary macro whose result is applied.
        "the implementation takes 0 argument(s) and the call site supplies 2",
        // A modifier with no name in the table.
        "a definition marked `DEFERRED`, a modifier scala-rs cannot rebuild yet",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err:?}"
        );
    }
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&out_dir);
}
