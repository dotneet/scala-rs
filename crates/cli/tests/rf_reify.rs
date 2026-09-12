//! `reify { … }` over blocks and over members of static `object`s
//! (`docs/macros.md` §7.17).
//!
//! Its own file rather than an addition to `engine.rs`, which is what the
//! per-slice files beside it (`libctor.rs`, `tqmacro.rs`, `engine.rs`) already
//! do; `reify.rs` is taken by an unrelated suite about dispatching to the
//! declaring class.
//!
//! Two kinds of check, because they catch different things.
//!
//! * `rf_shapes.scala` prints `showRaw` of each reified tree and is compared
//!   with real scalac 2.13.16. A reference built as a bare `Ident` instead of
//!   `Select(mkIdent(staticModule(…)), …)` still compiles and still evaluates
//!   to the same value wherever the name happens to be in scope, so **only the
//!   printed tree tells them apart** -- and only the second one keeps its
//!   meaning wherever the expansion lands.
//! * `rf_impl.scala` + `rf_use.scala` really expand through the JVM bridge in
//!   two runs, and are compared with the same two files built by real scalac.
//!   That is what says the tree does not merely print right but *runs*.
//!
//! `rf_more.scala` holds five bodies that were once refused and are reified now;
//! scala-rs refuses by name.

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
        "scala-rs-rfreify-{tag}-{}-{nanos}-{seq}",
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
/// cannot run them at all -- the same shape as `engine.rs`.
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

/// `showRaw` of every reified tree in `rf_shapes.scala`, against the runtime
/// universe.
#[test]
fn rf_shapes_reify_and_run() {
    if !prerequisites("rf_shapes") {
        return;
    }
    let out_dir = tmp_dir("rf_shapes");
    let out = compile("rf_shapes", &out_dir, &[]);
    assert!(
        out.status.success(),
        "compile rf_shapes failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&classpath(&[&out_dir]), "rf_shapes"),
        expected_stdout("rf_shapes"),
        "stdout mismatch for rf_shapes"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// The same file through real scalac 2.13.16. This is what makes the recorded
/// trees mean something: they are nsc's, not scala-rs's own invention.
#[test]
fn rf_shapes_match_real_scalac() {
    if !prerequisites("rf_shapes scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rf_shapes scalac diff: scalac not obtainable");
        return;
    };
    let out_dir = tmp_dir("rf_shapes-scalac");
    scalac_compile(&scalac, "rf_shapes", &out_dir, &[]);
    assert_eq!(
        run_main(&classpath(&[&out_dir]), "rf_shapes (real scalac build)"),
        expected_stdout("rf_shapes"),
        "recorded expectation for rf_shapes does not match real scalac"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// The macro path: `rf_impl.scala` is compiled first, and its `reify` bodies
/// are really expanded when `rf_use.scala` is compiled against it.
#[test]
fn rf_macros_expand_and_run() {
    if !prerequisites("rf_use") {
        return;
    }
    let impls = tmp_dir("rf_impl");
    let uses = tmp_dir("rf_use");
    let out = compile("rf_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile rf_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile("rf_use", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile rf_use failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&classpath(&[&uses, &impls]), "rf_use"),
        expected_stdout("rf_use"),
        "stdout mismatch for rf_use"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The same two files through real scalac 2.13.16. A block that dropped a
/// statement, or a splice built twice, would still compile and still run --
/// the count `rf_use` prints is what catches it.
#[test]
fn rf_macros_match_real_scalac() {
    if !prerequisites("rf_use scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rf_use scalac diff: scalac not obtainable");
        return;
    };
    let impls = tmp_dir("rf_impl-scalac");
    let uses = tmp_dir("rf_use-scalac");
    scalac_compile(&scalac, "rf_impl", &impls, &[]);
    scalac_compile(&scalac, "rf_use", &uses, &[&impls]);
    assert_eq!(
        run_main(&classpath(&[&uses, &impls]), "rf_use (real scalac build)"),
        expected_stdout("rf_use"),
        "recorded expectation for rf_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The five bodies `rf_bad.scala` once refused -- a member of the enclosing
/// `object`, a pattern `val`, a package-object function, a class definition,
/// a local of the enclosing method -- reified now that `reify` walks the
/// typed body by symbol (`docs/notes/reify-design.md`).
#[test]
fn rf_more_reifies_and_runs() {
    if !prerequisites("rf_more") {
        return;
    }
    let out_dir = tmp_dir("rf_more");
    let out = compile("rf_more", &out_dir, &[]);
    assert!(
        out.status.success(),
        "compile rf_more failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&classpath(&[&out_dir]), "rf_more"),
        expected_stdout("rf_more"),
        "stdout mismatch for rf_more"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// The same file through real scalac 2.13.16.
#[test]
fn rf_more_matches_real_scalac() {
    if !prerequisites("rf_more scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rf_more scalac diff: scalac not obtainable");
        return;
    };
    let out_dir = tmp_dir("rf_more-scalac");
    scalac_compile(&scalac, "rf_more", &out_dir, &[]);
    assert_eq!(
        run_main(&classpath(&[&out_dir]), "rf_more (real scalac build)"),
        expected_stdout("rf_more"),
        "recorded expectation for rf_more does not match real scalac"
    );
    let _ = fs::remove_dir_all(&out_dir);
}
