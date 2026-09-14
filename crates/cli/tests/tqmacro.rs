//! A macro def that exists only in a **compiled library's pickle**
//! (`agent/tq2`). `docs/macros.md` §5, `docs/gitbucket.md` root 18.
//!
//! A macro emits no bytecode, so the only record of one in a published jar is
//! the `ScalaSignature`: the `MACRO` flag plus the
//! `@scala.reflect.macros.internal.macroImpl` annotation naming the
//! implementation. Nothing read that, so `slick.lifted.TableQuery`'s
//! parameterless `def apply[E]: TableQuery[E]` was simply not a member, the
//! companion's *other* `apply[E](cons: Tag => E)` was the only one, and
//! gitbucket's `lazy val Issues = TableQuery[Issues]` came out as that
//! method's un-applied type -- 238 errors reading `value filter / insert /
//! join / map … is not a member of ((Tag) => Issues)TableQuery[Issues]`.
//!
//! Two compilations, the way nsc requires: `tq_mdef.scala` is compiled by
//! **real scalac** (only nsc writes that flag and that annotation), and
//! `tq_muse.scala` by scala-rs against its class files. The expansion really
//! runs -- the implementation is loaded from the class file and invoked
//! through the JVM bridge -- and the dual run against real scalac is what says
//! the tree that came back was the right one.
//!
//! `tq_muse_local.scala` is the same call with a type argument that is a class
//! *this* run is compiling -- gitbucket's shape. It used to be a diagnostic,
//! because the bridge rebuilds a type tag by name through a runtime mirror; it
//! now expands through a placeholder symbol (`docs/macros.md` §5.1), and
//! `crates/cli/tests/macrotag.rs` is where that expansion is checked against
//! real scalac.

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
        "scala-rs-tqmacro-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn tool_runs(name: &str) -> bool {
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

/// Everything this test needs, or a named skip.
fn prerequisites(tag: &str) -> bool {
    if !tool_runs("java") || !tool_runs("javac") {
        eprintln!("skip {tag}: java / javac not available");
        return false;
    }
    if scala_library_jar().is_none() || scala_reflect_jar().is_none() || find_scalac().is_none() {
        eprintln!("skip {tag}: the 2.13.16 toolchain is not obtainable");
        return false;
    }
    true
}

/// Compile `tq_mdef.scala` with real scalac. That is the whole point: its
/// pickle is the only place a macro def survives.
fn build_library() -> PathBuf {
    let scalac = find_scalac().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let out = tmp_dir("mdef");
    let res = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            fixtures_dir().join("tq_mdef.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        res.status.success(),
        "real scalac rejected tq_mdef.scala: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    out
}

fn compile_with_scala_rs(name: &str, out: &Path, lib: &Path) -> std::process::Output {
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
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
            &format!("{}:{}", lib.display(), reflect.display()),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile")
}

fn compile_without_reflect(name: &str, out: &Path, lib: &Path) -> std::process::Output {
    let jar = scala_library_jar().unwrap();
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
            lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile")
}

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

/// The whole path: the macro def is read from the library's pickle, the two
/// `apply` alternatives are told apart by position, and the implementation is
/// really run.
#[test]
fn tq_pickled_macro_expands_and_runs() {
    if !prerequisites("tq_muse") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let lib = build_library();
    let uses = tmp_dir("muse");

    let out = compile_with_scala_rs("tq_muse", &uses, &lib);
    assert!(
        out.status.success(),
        "compile tq_muse failed: {}",
        diagnostics(&out)
    );
    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        lib.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "tq_muse"),
        expected_stdout("tq_muse"),
        "stdout mismatch for tq_muse"
    );
    let _ = fs::remove_dir_all(&uses);
    let _ = fs::remove_dir_all(&lib);
}

/// The same two files through real scalac, which is what makes the recorded
/// expectation mean anything: a macro that expanded to something else would
/// still compile and still run.
#[test]
fn tq_pickled_macro_matches_real_scalac() {
    if !prerequisites("tq_muse scalac diff") {
        return;
    }
    let scalac = find_scalac().unwrap();
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let lib = build_library();
    let uses = tmp_dir("muse-scalac");

    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", lib.display(), reflect.display()),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("tq_muse.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected tq_muse.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        lib.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "tq_muse (real scalac build)"),
        expected_stdout("tq_muse"),
        "recorded expectation for tq_muse does not match real scalac"
    );
    let _ = fs::remove_dir_all(&uses);
    let _ = fs::remove_dir_all(&lib);
}

/// A type argument the *current* run defines: gitbucket's own shape, which the
/// bridge used to refuse. It now expands through a placeholder symbol.
///
/// Only compilation is checked here. The program is not run, because
/// `TqLocal.label` reads a constructor `val` of a class in another compilation
/// unit and that is a separate code-generation bug; the dual run against real
/// scalac lives in `crates/cli/tests/macrotag.rs`.
#[test]
fn tq_type_argument_from_this_run_expands() {
    if !prerequisites("tq_muse_local") {
        return;
    }
    let lib = build_library();
    let out_dir = tmp_dir("muselocal");
    let out = compile_with_scala_rs("tq_muse_local", &out_dir, &lib);
    assert!(
        out.status.success(),
        "expected tq_muse_local to compile: {}",
        diagnostics(&out)
    );
    let _ = fs::remove_dir_all(&out_dir);
    let _ = fs::remove_dir_all(&lib);
}

/// An inherited abstract type member in a companion module is part of a
/// pickled macro's result type. It must be retained when the macro candidate
/// is reconstructed; otherwise the candidate disappears and the caller gets
/// a misleading "no implicit" diagnostic. The fixture deliberately omits
/// scala-reflect from the consumer classpath, so the test stops immediately
/// after candidate discovery with the known macro-engine diagnostic.
#[test]
fn pickled_macro_with_module_inherited_type_is_supplied() {
    if !prerequisites("tracer_mdef") {
        return;
    }
    let scalac = find_scalac().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let lib = tmp_dir("tracer-mdef");
    let built = Command::new(scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            lib.to_str().unwrap(),
            fixtures_dir().join("tracer_mdef.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        built.status.success(),
        "real scalac rejected tracer_mdef.scala: {}",
        diagnostics(&built)
    );

    let out_dir = tmp_dir("tracer-muse");
    let out = compile_without_reflect("tracer_muse", &out_dir, &lib);
    let text = diagnostics(&out);
    assert!(
        !out.status.success(),
        "expected missing scala-reflect to stop expansion"
    );
    assert!(
        text.contains("macro expansion is not implemented"),
        "expected macro candidate to be supplied, got: {text}"
    );
    assert!(
        !text.contains("no implicit"),
        "macro candidate was lost during pickle reconstruction: {text}"
    );
    let _ = fs::remove_dir_all(&out_dir);
    let _ = fs::remove_dir_all(&lib);
}
