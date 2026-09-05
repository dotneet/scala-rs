//! E2E tests for the `agent/slickrun` slice (fixture prefix `slickrun`).
//!
//! This slice's question was not "does slick compile?" -- it already did, with
//! every one of its 4552 class files loading under `-Xverify:all` -- but "does
//! what came out actually *run*?". `tests/slick_run.sh` answers it by compiling
//! slick twice (scala-rs and real scalac), compiling a set of ordinary slick
//! client programs *once* with scalac, and running that one client binary
//! against each slick build. Every difference is therefore slick's class
//! files, i.e. scala-rs.
//!
//! The fixture collects the shapes that broke, each of which was a run-time
//! failure of code that had type-checked and verified in isolation:
//!
//!  1. a nested `def` whose `match` binds names -- the binders were counted as
//!     free variables, so the lifted method grew parameters for them and the
//!     enclosing trait declared capture accessors no class could implement
//!     (`throw new RuntimeException("cannot capture cons for trait
//!     JdbcProfile")`);
//!  2. `Box[_ <: E].value` -- an existential's skolem erases to its upper
//!     bound, and without the `checkcast` slick's `def baseTableRow: E =
//!     shaped.value` failed verification, killing every `TableQuery`;
//!  3. a case class's companion running the `$init$` of the traits the *class*
//!     mixes in (`IncompatibleClassChangeError` on `slick.ast.Apply$`);
//!  4. `length.compare(n)` reaching `Ordered` through a view that was never
//!     materialised, leaving a `checkcast scala/math/Ordered` on an `int`;
//!  5. an auxiliary constructor of an inner class missing its `$outer`
//!     parameter (`NoSuchMethodError` on the first `Table` subclass);
//!  6. a tuple result of `FunctionN.apply` left uncast;
//!  7. a trait `val` overridden by a narrower one in a derived trait -- the
//!     base trait's mixin setter and the getter bridge were both missing
//!     (`AbstractMethodError` initialising `H2Profile$`);
//!  8. a default argument on a *trait* method with no `name$default$n` getter
//!     anywhere;
//!  9. a `private` constructor the companion calls, emitted `ACC_PRIVATE`;
//! 10. a `case class` pattern naming only the first of several parameter lists,
//!     which took the extractor path to a companion `unapply` that is a symbol
//!     with no method behind it;
//! 11. `if (c) e` with no `else` and a non-`Unit` branch value, whose two paths
//!     met the join at different stack heights.
//!
//! Self-contained (own copies of the small helpers `e2e.rs` also has) per
//! `.agent-brief.md`: the shared test files belong to other in-flight agents.

use std::fs;
use std::path::PathBuf;
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
        "scala-rs-slickrun-{tag}-{}-{nanos}-{seq}",
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
    out
}

/// Compile against the real jar and run under `-Xverify:all`. Every case in
/// the fixture is a *verification or linkage* failure when it regresses, so
/// the run is the test; the recorded expectation is real scalac 2.13.16's own
/// stdout for the same file.
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
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp out:scala-library failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_slickrun_dual_run() {
    dual_run_fixture("slickrun");
}

/// The four `{ case … }` literals in `PatLambdas` / `PatInTrait` are lowered to
/// hoisted `$anonfun$` statics, not to closure classes, and the run above shows
/// their pattern binders, the captured enclosing `this` and a captured `var`
/// all still line up in that static method's locals.
///
/// This is the shape `agent/slickrun` first saw fail — but the cause was a
/// merge artifact in `load_this`, not the lowering. Pinned here so the next
/// slice that widens the indy boundary (a `{ case … }` where a plain
/// `Function1` is expected) has coverage for it rather than a rumour.
#[test]
fn fixtures_slickrun_pattern_lambdas_are_hoisted() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let out = compile_fixture_with("slickrun", &["--scala-library", jar.to_str().unwrap()]);
    let names: Vec<String> = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let lambda_classes: Vec<&String> = names.iter().filter(|n| n.contains("anonfun")).collect();
    assert!(
        lambda_classes.is_empty(),
        "a lambda became a class file: {lambda_classes:?}"
    );
    // The only anonymous *classes* here are the two the source writes out --
    // `new Comp {}` and `new HasSubOpts {}`. If a `{ case … }` regresses to the
    // closure-class path this number moves, so move it deliberately.
    let anon: Vec<&String> = names.iter().filter(|n| n.contains("$$anon$")).collect();
    assert_eq!(
        anon.len(),
        2,
        "expected exactly two anonymous classes, got {anon:?}"
    );
    for cls in ["PatLambdas.class", "PatInTrait.class"] {
        let bytes = fs::read(out.join(cls)).unwrap_or_else(|e| panic!("{cls}: {e}"));
        let needle = b"$anonfun$";
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle),
            "{cls} has no hoisted $anonfun$ method"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
