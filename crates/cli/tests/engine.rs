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

fn compile_with_tmpdir(
    name: &str,
    out: &Path,
    extra: &[&Path],
    tmpdir: &Path,
) -> std::process::Command {
    let jar = scala_library_jar().expect("scala-library");
    let reflect = scala_reflect_jar().expect("scala-reflect");
    let mut cp = reflect.display().to_string();
    for e in extra {
        cp.push(':');
        cp.push_str(&e.display().to_string());
    }
    let mut command = Command::new(bin());
    command.env("TMPDIR", tmpdir).args([
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
    ]);
    command
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

/// Two compiler processes may populate the same engine cache concurrently.
/// The cache must expose either the old complete class or the new complete
/// class, never a source directory whose class file is still being written.
#[test]
fn concurrent_engine_cache_publication_is_safe() {
    if !prerequisites("concurrent engine cache") {
        return;
    }
    let impls = tmp_dir("cache-race-impl");
    let cache = tmp_dir("cache-race-tmp");
    let output = compile("eg_impl", &impls, &[]);
    assert!(
        output.status.success(),
        "compile eg_impl failed: {}",
        diagnostics(&output)
    );

    let mut children = Vec::new();
    let mut outputs = Vec::new();
    let mut dirs = Vec::new();
    for n in 0..2 {
        let out = tmp_dir(&format!("cache-race-use-{n}"));
        let mut command = compile_with_tmpdir("eg_use", &out, &[&impls], &cache);
        children.push(command.spawn().expect("spawn concurrent scala-rs compile"));
        dirs.push(out);
    }
    for child in children {
        outputs.push(
            child
                .wait_with_output()
                .expect("wait for concurrent compile"),
        );
    }
    for output in outputs {
        assert!(
            output.status.success(),
            "concurrent eg_use compile failed: {}",
            diagnostics(&output)
        );
    }

    let mut classes = Vec::new();
    for entry in fs::read_dir(&cache).expect("read cache race directory") {
        let path = entry.expect("cache entry").path();
        let class = path.join("ScalaRsMacroEngine.class");
        if class.is_file() {
            let bytes = fs::read(&class).expect("read published engine class");
            assert!(bytes.len() >= 8, "published engine class is truncated");
            assert_eq!(&bytes[0..4], &[0xca, 0xfe, 0xba, 0xbe]);
            assert_eq!(u16::from_be_bytes([bytes[6], bytes[7]]), 52);
            classes.push(class);
        }
    }
    assert_eq!(classes.len(), 1, "cache publication left duplicate classes");

    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&cache);
    for dir in dirs {
        let _ = fs::remove_dir_all(dir);
    }
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

/// The stage-D forms that are refused, each by name.
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
    // A modifier with no name in the table.
    let needle = "a definition marked `DEFERRED`, a modifier scala-rs cannot rebuild yet";
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics, got {err:?}"
    );
    // `SdGaps.query[SdLocalRow]` in the same file is *not* a gap any more: a
    // class this run compiles reaches the implementation as a placeholder
    // symbol (`docs/macros.md` §5.1, `crates/cli/tests/macrotag.rs`). It is
    // left there as the near miss it now is -- the file has exactly one error.
    assert!(
        !err.contains("SdLocalRow"),
        "expected the local row class to expand, got {err:?}"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&out_dir);
}

// ---------------------------------------------------------------------------
// The two gaps in front of a self-built `reify`: a nested `object` of the
// reflection API, and `<a pickled val>.type` as a stable identifier.
// `docs/macros.md` §7.8 residuals 5 and 6, §7.13.4 gaps 1 and 2.

/// `tests/fixtures/rd_nested.scala`, compiled by scala-rs and run.
///
/// Every line of it drew a diagnostic before: "value Expr is not a member of
/// Universe", "not found: value Expr", "stable identifier required, but
/// scala.reflect.runtime.universe found". *Running* it is what makes the test
/// mean something -- a member `object` reached through the wrong receiver
/// compiles perfectly well and throws `ClassCastException` at the first call.
#[test]
fn rd_nested_objects_and_stable_paths_run() {
    if !prerequisites("rd_nested") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let out_dir = tmp_dir("rd_nested");
    let out = compile("rd_nested", &out_dir, &[]);
    assert!(
        out.status.success(),
        "compile rd_nested failed: {}",
        diagnostics(&out)
    );
    let cp = format!(
        "{}:{}:{}",
        out_dir.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "rd_nested"),
        expected_stdout("rd_nested"),
        "stdout mismatch for rd_nested"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// The same file through real scalac 2.13.16: the recorded expectation is
/// nsc's output, not scala-rs's.
#[test]
fn rd_nested_matches_real_scalac() {
    if !prerequisites("rd_nested scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rd_nested scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let out_dir = tmp_dir("rd_nested-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            out_dir.to_str().unwrap(),
            fixtures_dir().join("rd_nested.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected rd_nested.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cp = format!(
        "{}:{}:{}",
        out_dir.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "rd_nested (real scalac build)"),
        expected_stdout("rd_nested"),
        "recorded expectation for rd_nested does not match real scalac"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// `rd_impl.scala` + `rd_use.scala`: the shape `reify { … }` expands into,
/// written out by hand and **expanded for real** through the bridge.
///
/// `reify` itself is still the §7.8 diagnostic, but everything it has to emit
/// is exercised here: `c.universe.Expr.apply` (whose pickled signature says
/// `Mirror[Universe.this.type]` and is written out, the way `TypeTag.apply`
/// is), `Mirror[c.universe.type]`, a `TreeCreator` subclass, a static symbol
/// resolved through `mirror.staticModule`, and a splice through `Expr.in`.
#[test]
fn rd_reify_shape_expands_and_runs() {
    if !prerequisites("rd_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("rd_impl");
    let uses = tmp_dir("rd_use");

    let out = compile("rd_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile rd_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile("rd_use", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile rd_use failed: {}",
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
        run_main(&cp, "rd_use"),
        expected_stdout("rd_use"),
        "stdout mismatch for rd_use"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The same two files through real scalac 2.13.16. A creator that resolved
/// the static symbol in the wrong universe, or spliced the argument's tree
/// without rebasing it, would still compile -- only the output would differ.
#[test]
fn rd_reify_shape_matches_real_scalac() {
    if !prerequisites("rd_use scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rd_use scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("rd_impl-scalac");
    let uses = tmp_dir("rd_use-scalac");

    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            impls.to_str().unwrap(),
            fixtures_dir().join("rd_impl.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected rd_impl.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", reflect.display(), impls.display()),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("rd_use.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected rd_use.scala: {}",
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
        run_main(&cp, "rd_use (real scalac build)"),
        expected_stdout("rd_use"),
        "recorded expectation for rd_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// `rb_impl.scala` + `rb_use.scala`: **`reify { … }` expanded by scala-rs**
/// (`docs/macros.md` §7.14, `crates/typer/src/reify_expand.rs`).
///
/// `rd_impl.scala` above writes out, by hand, the tree `reify` has to build;
/// this pair writes `reify` and makes the compiler build it. Sixteen lines of
/// output cover the four stages: a literal body, a static `object` reached
/// through `mirror.staticModule`, `.splice` rebased through `Expr.in`, and a
/// type argument rebuilt from `staticClass` or from the tag in scope (the
/// shape slick's `TableQueryMacroImpl` needs) -- including two splices whose
/// side effects say each was evaluated once.
#[test]
fn rb_reify_expands_and_runs() {
    if !prerequisites("rb_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("rb_impl");
    let uses = tmp_dir("rb_use");

    let out = compile("rb_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile rb_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile("rb_use", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile rb_use failed: {}",
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
        run_main(&cp, "rb_use"),
        expected_stdout("rb_use"),
        "stdout mismatch for rb_use"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The same two files through real scalac 2.13.16. A reified body that
/// resolved `RbHelper` in the wrong universe, or spliced an argument's tree
/// without rebasing it, would still compile and still run -- only the output
/// would differ, which is why the comparison is of output.
#[test]
fn rb_reify_matches_real_scalac() {
    if !prerequisites("rb_use scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rb_use scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("rb_impl-scalac");
    let uses = tmp_dir("rb_use-scalac");

    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            impls.to_str().unwrap(),
            fixtures_dir().join("rb_impl.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected rb_impl.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", reflect.display(), impls.display()),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("rb_use.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected rb_use.scala: {}",
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
        run_main(&cp, "rb_use (real scalac build)"),
        expected_stdout("rb_use"),
        "recorded expectation for rb_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The bodies `reify` refuses, each named. Real scalac accepts all five (it
/// reifies a local as a *free term*, and its type reifier does not need a tag
/// in scope); scala-rs does not build those, and says so rather than reifying
/// the bare name -- which would compile, run, and mean whatever stood at the
/// call site.
#[test]
fn rb_reify_gaps_are_named() {
    if !prerequisites("rb_bad") {
        return;
    }
    let out_dir = tmp_dir("rb_bad");
    let out = compile("rb_bad", &out_dir, &[]);
    assert!(!out.status.success(), "rb_bad.scala should not compile");
    let text = diagnostics(&out);
    for want in [
        "`x` is a local, a parameter, or a name that does not stand for a static `object`",
        "`n` is a local, a parameter, or a name that does not stand for a static `object`",
        "a type ascription is not reified yet",
        // A block, and an ordinary `val` bound inside one, are reified since
        // §7.17 and the `agent/reifydefs` slice; what `useBlock` is refused
        // for is the *pattern* `val` it binds, a different tree in nsc.
        "a pattern definition (`val (a, b) = ...`) is not reified yet",
        "a type argument cannot be rebuilt: `T`, an abstract type with no tag in scope",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    assert!(
        text.contains("cannot expand reify { ... }"),
        "the report should name reify:\n{text}"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// slick's `ShapedValue.mapToImpl`, taken apart (`docs/macros.md` §7.16): a
/// macro implementation whose `Context` is refined with a `PrefixType`, a
/// field walk over `rTag.tpe.decls`, and `..$` splices among ordinary
/// elements in an argument clause, a block and a template body.
#[test]
fn sv_refined_context_and_mixed_splices_run() {
    if !prerequisites("sv_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("sv_impl");
    let uses = tmp_dir("sv_use");

    let out = compile("sv_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile sv_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile("sv_use", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile sv_use failed: {}",
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
        run_main(&cp, "sv_use"),
        expected_stdout("sv_use"),
        "stdout mismatch for sv_use"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The same two files through real scalac 2.13.16. The expansion carries the
/// *printed* form of a template whose body mixes a `..$` splice with an
/// ordinary member, so a concatenation that reordered the pieces -- which
/// still compiles and still runs -- shows up as a different line.
#[test]
fn sv_refined_context_and_mixed_splices_match_real_scalac() {
    if !prerequisites("sv_use scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip sv_use scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("sv_impl-scalac");
    let uses = tmp_dir("sv_use-scalac");

    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            impls.to_str().unwrap(),
            fixtures_dir().join("sv_impl.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected sv_impl.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", reflect.display(), impls.display()),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("sv_use.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected sv_use.scala: {}",
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
        run_main(&cp, "sv_use (real scalac build)"),
        expected_stdout("sv_use"),
        "recorded expectation for sv_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The three forms still refused. Two of them real scalac rejects as well, so
/// they pin agreement rather than a gap; the third (a `case` class whose
/// parents are a splice) nsc reifies and scala-rs does not.
#[test]
fn sv_refused_forms_are_named() {
    if !prerequisites("sv_gaps_bad") {
        return;
    }
    let out_dir = tmp_dir("sv_gaps_bad");
    let out = compile("sv_gaps_bad", &out_dir, &[]);
    assert!(
        !out.status.success(),
        "sv_gaps_bad.scala should not compile"
    );
    let text = diagnostics(&out);
    for want in [
        "a rank-2 hole (...$xss) cannot stand for a list of trees",
        "a `case` class whose parents are a `..$` splice is not reified yet",
        "must take scala.reflect.macros.blackbox.Context",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    let _ = fs::remove_dir_all(&out_dir);
}

// ---------------------------------------------------------------------------
// `scala.reflect.runtime` *visibility*: `TypeTag` / `WeakTypeTag` /
// `Transformer` as types, and `JavaUniverse#runtimeMirror`.
//
// Unlike the `eg_*` / `ex_*` / `rd_*` / `rb_*` families above, nothing here
// runs the macro engine: these are ordinary jar members (a nested trait, an
// abstract class, a plain method) that were never *installed* at all, so
// every reference to them was "not found" or "not a member" regardless of
// what the member actually does. `PickleSupply::complete_type_member` did not
// have a case for `MemberKind::Class` (a nested class or trait named as a
// *type*, as opposed to a type alias or an abstract type member), and
// `java.lang.ClassLoader` had no symbol at all outside the classfile loader's
// on-demand path, which `JavaUniverse#runtimeMirror`'s parameter needed
// before the method could even be considered. See `crates/typer/src/
// pickle_supply.rs`'s `complete_type_member_uncached` and `crates/typer/src/
// prelude_reflectruntime.rs`.

/// `tests/fixtures/rt_typetags.scala`: `TypeTag[Int]` / `WeakTypeTag[String]`
/// as types (`u.TypeTag[T]` / `u.WeakTypeTag[T]`, nested inside trait
/// `TypeTags`), `Transformer` as a type (nested inside trait `Trees`, an
/// *abstract class* rather than a trait -- confirming the fix is not
/// specific to interfaces), and `runtimeMirror(ClassLoader)`.
///
/// The tag values print through `.tpe.toString` rather than directly: nsc's
/// own `WeakTypeTag` materialiser upgrades a concrete type to a full
/// `TypeTag` regardless of which one was asked for (`implicitly[WeakTypeTag
/// [Int]]` prints `TypeTag[Int]`, confirmed against real scalac), which is a
/// materialisation nuance this slice does not touch; comparing `.tpe` isolates
/// what was actually fixed here -- that the *names* resolve at all -- from
/// that separate, pre-existing difference.
#[test]
fn rt_typetags_resolve_and_run() {
    if !prerequisites("rt_typetags") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let out_dir = tmp_dir("rt_typetags");
    let out = compile("rt_typetags", &out_dir, &[]);
    assert!(
        out.status.success(),
        "compile rt_typetags failed: {}",
        diagnostics(&out)
    );
    let cp = format!(
        "{}:{}:{}",
        out_dir.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "rt_typetags"),
        expected_stdout("rt_typetags"),
        "stdout mismatch for rt_typetags"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// The same file through real scalac 2.13.16.
#[test]
fn rt_typetags_matches_real_scalac() {
    if !prerequisites("rt_typetags scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rt_typetags scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let out_dir = tmp_dir("rt_typetags-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            out_dir.to_str().unwrap(),
            fixtures_dir().join("rt_typetags.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected rt_typetags.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cp = format!(
        "{}:{}:{}",
        out_dir.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "rt_typetags (real scalac build)"),
        expected_stdout("rt_typetags"),
        "recorded expectation for rt_typetags does not match real scalac"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// `tests/fixtures/rt_currentmirror.scala`: `currentMirror` really expands.
///
/// It is one of nsc's *fast-track* macros -- the compiler recognises it by the
/// macro symbol's full name and supplies the expansion itself, because the
/// `@macroImpl` annotation on the real classfile is the placeholder `???` and
/// there is no implementation to invoke. scala-rs does the same, in
/// `crates/typer/src/fasttrack_mirror.rs`; this file used to be
/// `rt_currentmirror_bad.scala`, a confession that the name was visible but no
/// reference to it could be expanded.
///
/// Compared against real scalac 2.13.16, because an expansion that reached the
/// wrong class loader would still compile and still run.
#[test]
fn rt_currentmirror_expands_and_runs() {
    if !prerequisites("rt_currentmirror") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let out_dir = tmp_dir("rt_currentmirror");
    let out = compile("rt_currentmirror", &out_dir, &[]);
    assert!(
        out.status.success(),
        "compile rt_currentmirror failed: {}",
        diagnostics(&out)
    );
    let cp = format!(
        "{}:{}:{}",
        out_dir.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "rt_currentmirror"),
        expected_stdout("rt_currentmirror")
    );
    let _ = fs::remove_dir_all(&out_dir);

    let Some(scalac) = find_scalac() else {
        eprintln!("skip rt_currentmirror scalac diff: scalac not obtainable");
        return;
    };
    let scalac_out = tmp_dir("rt_currentmirror-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            scalac_out.to_str().unwrap(),
            fixtures_dir()
                .join("rt_currentmirror.scala")
                .to_str()
                .unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected rt_currentmirror.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cp = format!(
        "{}:{}:{}",
        scalac_out.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "rt_currentmirror (real scalac build)"),
        expected_stdout("rt_currentmirror"),
        "recorded expectation for rt_currentmirror does not match real scalac"
    );
    let _ = fs::remove_dir_all(&scalac_out);
}
