//! E2E tests for the `agent/hkunify` slice: partial unification of a
//! higher-kinded type variable.
//!
//! `traverse(fa)(a => State(s => f(s, a)))` has to solve `G[B]` against
//! `IndexedStateT[Eval, S, S, B]`, a four-parameter type applied to four
//! arguments. nsc (`TypeVar.unifyFull`, scala/bug#2712) reads the constructor
//! as curried: the leftmost surplus arguments are captured as constants and
//! the rightmost ones are abstracted, `G := IndexedStateT[Eval, S, S, *]`. A
//! partially applied class is already a type constructor in this compiler's
//! representation, so no lambda symbol has to be invented; `unify_one_precise`
//! captures the surplus, `collect_expected` does the same for an invariant or
//! contravariant position of the expected type, and a literal's parameter
//! whose expected type is still a variable is read off the literal's body the
//! way nsc's `typedFunctionUndoingEtaExpansion` does.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `hku` prefix. The positive fixture
//! is exercised against the real scala-library only: it uses `Either`,
//! `foldRight` and `List.empty`, which the private runtime does not carry
//! (the same arrangement as `wci_open`).

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
        "scala-rs-hkunify-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    None
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
        "compile {name} failed extra={extra:?}:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// `-Xverify:all`: a constructor captured with the wrong arguments would
/// erase to the wrong class and show up here, not as a silent difference.
fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
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
    assert_eq!(
        run_java(&out, Some(jar_s), "Main"),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
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
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------- positive: it runs

/// Every line prints the name of the type an implicit was found for, so the
/// solution itself is what is compared: `pick(e: Either[String, Int])` prints
/// `Int` because `A := Int` and `F := Either[String, *]`, `two(new Tri[Int,
/// String, Boolean])` prints `String,Boolean` because the rightmost two are
/// the abstracted ones, and `mapAccumulate` is cats' own line 143. On the
/// pre-fix binary the fixture does not compile at all: 17 errors, every one
/// of the shapes above among them.
#[test]
fn fixtures_hku_partial() {
    dual_run_fixture("hku_partial");
}

/// The fixture is only worth its runtime if real scalac accepts it too -- and
/// prints exactly what `expected/hku_partial.txt` records.
#[test]
fn scalac_agrees_hku_partial_runs() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-hku-partial");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("hku_partial.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected hku_partial.scala: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        expected_stdout("hku_partial"),
        "scalac's own run does not match the recorded output"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------ negative: only the rightmost

/// Each of these would type-check under some other abstraction than the one
/// nsc picks: `[x]Either[x, Int]` would let `both(e, new Inv[String])`
/// through, the expected type asks for the other half of `Either`, a
/// two-parameter variable meets a one-parameter type, and the rightmost
/// parameter of `Foo[A, M[_]]` is a constructor rather than a proper type.
#[test]
fn fixtures_hku_partial_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not available");
        return;
    };
    compile_fails(
        "hku_partial_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "no matching overload for (F[A], Inv[A])Int with arguments (Either[String, Int], Inv[String])",
            "hku_partial_bad.scala:16",
            "type mismatch; found: Either[String, Int]  required: Either[Int, String]",
            "hku_partial_bad.scala:19",
            "no matching overload for (F[A, B])Int with arguments (Option[Int])",
            "hku_partial_bad.scala:22",
            "no matching overload for (G[A])Int with arguments (Foo[Int, List])",
            "hku_partial_bad.scala:26",
        ],
    );
}

/// The same four, straight from scalac, at the same lines, so the fixture
/// cannot drift into asserting a restriction nsc does not have.
#[test]
fn scalac_agrees_hku_partial_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-hku-partial-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("hku_partial_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(
        !output.status.success(),
        "scalac accepted hku_partial_bad.scala"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "hku_partial_bad.scala:16: error: type mismatch",
        "required: Main.Inv[Any]",
        "hku_partial_bad.scala:19: error: type mismatch",
        "required: Either[Int,String]",
        "hku_partial_bad.scala:22: error: no type parameters for method two",
        "hku_partial_bad.scala:26: error: no type parameters for method hk",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac's error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
