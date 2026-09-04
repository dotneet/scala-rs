//! E2E tests for the `agent/testkit` slice: compiling slick's own test suite
//! (`slick-testkit`) against the 4552 classfiles scala-rs produced for slick.
//!
//! Four roots, in the order they stopped the measurement:
//!
//! * **`expected pattern, found case`** (`JdbcMapperTest.scala:443`, `:506`).
//!   `for (case p <- xs)` is Scala 3's spelling of a filtering generator, and
//!   scalac 2.13.16 accepts the `case` marker with no `-Xsource` flag at all.
//!   Parse errors abort before typing, so the first measurement's "8 errors"
//!   was the parser's, not the typer's -- the real figure behind it was 2112.
//!
//! * **a guard on a destructuring generator saw nothing the pattern bound.**
//!   `for ((i, s) <- xs if i > 0)` built its `withFilter` closure from
//!   `pat.name()`, which is `None` for a tuple pattern, so the parameter was
//!   `_` and `i` was "not found: value i". Pre-existing and unrelated to the
//!   `case` marker; found while dual-running the fix for it.
//!
//! * **`import tdb.profile.api.*` resolved nothing** -- 1141 of those 2112
//!   errors (`not found: value column`, `not found: type Table`, `value O`,
//!   `type Rep`, `value DBIO`, ...). An import is typed once per pass and the
//!   first pass runs before the enclosing template's `val`s have signatures;
//!   `type_select` retypes a qualifier only while it is still `NoType`, so a
//!   *two-or-more-segment* prefix that failed on pass one kept its `Error`
//!   forever. `import d.p._` (an `Ident` qualifier, always retyped) recovered
//!   on pass four and `import d.p.api._` never did -- which is why the shape
//!   every slick test is written in was the one that broke. The prefix is now
//!   cleared before each retry, and a prefix that later resolves retracts the
//!   provisional diagnostics an earlier pass filed against it.
//!
//! * **scala-rs's classfiles said "extends Object and nothing else".**
//!   `CLASSINFOtpe` was written with `java.lang.Object` as its only parent, so
//!   no inherited member survived into a later compilation. Real scalac
//!   reading scala-rs's slick output reported the same errors this compiler
//!   did (`value api is not a member of object H2Profile`), which is what
//!   identified the writer rather than the reader. Fixed together with three
//!   things in the same signature that the testkit needs: a parameterised
//!   `type Rep[T] = ...` was pickled without its parameters, an abstract
//!   `type API <: Api` was pickled as `Nothing .. Any`, and `val L = List`
//!   named `<root>.List`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with other
//! agents; see `.agent-brief.md`. All fixtures use the `testkit` prefix.

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
        "scala-rs-testkit-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn compile_fixtures_with(tag: &str, names: &[&str], extra: &[&str]) -> PathBuf {
    let out = tmp_dir(tag);
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for n in names {
        cmd.arg(fixtures_dir().join(format!("{n}.scala")));
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {names:?} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn run_java(out: &Path, main: &str, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(e) => format!("{}:{}", out.display(), e),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `for (case p <- xs)`, a guard on a destructuring generator, and the two
/// import-prefix shapes, run against the real scala-library.
#[test]
fn fixtures_testkit_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run testkit: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixtures_with("testkit", &["testkit"], &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, "Main", Some(jar_s)),
        expected_stdout("testkit"),
        "stdout mismatch for library-ABI testkit"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac 2.13.16. Worth stating explicitly:
/// scalac accepts `for (case p <- xs)` with **no** `-Xsource:3`, so the
/// parser change is not gated on the flag either.
#[test]
fn real_scalac_dual_run_testkit() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff testkit: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("testkit.scala");
    let ref_out = tmp_dir("testkit-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile testkit");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, "Main", Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout("testkit"),
        "recorded expectation for testkit does not match real scalac"
    );
    let out = compile_fixtures_with("testkit", &["testkit"], &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, "Main", Some(jar_s)),
        reference,
        "stdout differs from real scalac for testkit"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// Separate compilation: `testkit_use.scala` sees only the *classfiles* of
/// `testkit_lib.scala`. Before the `CLASSINFOtpe` fix this failed with
/// "not found: value greeting" -- every inherited member was invisible.
#[test]
fn fixtures_testkit_separate_compilation() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip separate-compilation testkit: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let lib = compile_fixtures_with("testkitlib", &["testkit_lib"], &["--scala-library", jar_s]);
    let lib_s = lib.to_str().unwrap().to_string();
    let usr = compile_fixtures_with(
        "testkituse",
        &["testkit_use"],
        &["--scala-library", jar_s, "-cp", &lib_s],
    );
    assert_eq!(
        run_java(&usr, "testkitlib.Main", Some(&format!("{lib_s}:{jar_s}"))),
        expected_stdout("testkit_use"),
        "stdout mismatch for separately compiled testkit_use"
    );
    let _ = fs::remove_dir_all(&usr);
    let _ = fs::remove_dir_all(&lib);
}

/// The differential that identified the bug: real scalac compiling against
/// scala-rs's classfiles. If our `ScalaSignature` is wrong, nsc says so.
#[test]
fn real_scalac_reads_scala_rs_classfiles() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip scalac-reads-ours testkit: scalac or jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let lib = compile_fixtures_with("testkitlib2", &["testkit_lib"], &["--scala-library", jar_s]);
    let lib_s = lib.to_str().unwrap().to_string();
    let usr = tmp_dir("testkituse-scalac");
    let output = Command::new(&scalac)
        .args([
            fixtures_dir().join("testkit_use.scala").to_str().unwrap(),
            "-cp",
            &lib_s,
            "-d",
            usr.to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        output.status.success(),
        "real scalac could not read scala-rs's classfiles: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        run_java(&usr, "testkitlib.Main", Some(&format!("{lib_s}:{jar_s}"))),
        expected_stdout("testkit_use"),
        "scalac-compiled user of scala-rs's classfiles printed the wrong thing"
    );
    let _ = fs::remove_dir_all(&usr);
    let _ = fs::remove_dir_all(&lib);
}
