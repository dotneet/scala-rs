//! E2E tests for the `agent/anyconstr` slice: a type-constructor alias whose
//! body does not mention its parameter.
//!
//! `scala.collection` declares `type AnyConstr[X] = Any`. It is a type
//! *constructor*, so `IterableOps[A, CC, C]` really is an `IterableOps[A,
//! AnyConstr, _]` for every `CC` -- but only once the alias is reduced. nsc
//! does that in `isHKSubType`: normalize both constructors (which eta-expands
//! each to a `PolyType` and beta-reduces an alias body), then `isPolySubType`
//! asks `sameLength` on the parameters and compares the bodies. This compiler
//! already compared two eta-expandable constructors that way
//! (`SymbolTable::eta_expand_pair`, `agent/hkunify`); what it declined was the
//! pair where one side is an *abstract* constructor -- a higher-kinded type
//! parameter or an abstract type member -- which is exactly the library's
//! case.
//!
//! The added arm is deliberately widening-only: where both sides eta-expand,
//! their bodies are the whole answer, but an abstract constructor's body is
//! `CC[x]` and settles nothing, so a `false` there must still fall through to
//! the bound, path-member and projection arms below it.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. Fixtures use the `ac_` prefix and are exercised against
//! the real scala-library only (they name `List`, `Vector` and `Option`).

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
        "scala-rs-anyconstr-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: an alias reduced to the wrong body would erase the call to
/// the wrong descriptor and show up here rather than as a silent difference.
fn run_java(out: &Path, cp_extra: &str, main: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
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

/// Every line prints the name of the method body that ran, so an acceptance
/// that answered the conformance question the *wrong* way cannot pass. The
/// load-bearing lines are the `sel:` ones: `sel` has an
/// `Ops[Int, AnyConstr, _]` overload beside an `Any` one, and on the pre-fix
/// binary `sel(this)` from inside a trait with an abstract `CC` compiled
/// silently and printed `sel:any` where scalac prints `sel:anyconstr`. The
/// three `anyOps`/`stillAnyOps` calls through an abstract constructor did not
/// compile at all before the fix.
#[test]
fn fixtures_ac_anyconstr_runs() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ac_anyconstr", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, jar_s, "Main"),
        expected_stdout("ac_anyconstr"),
        "stdout mismatch for library dual-run ac_anyconstr"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `expected/ac_anyconstr.txt` is real scalac 2.13.16's own run of the same
/// source, so the fixture cannot drift into recording this compiler's answer.
#[test]
fn scalac_agrees_ac_anyconstr_runs() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-ac-anyconstr");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("ac_anyconstr.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected ac_anyconstr.scala: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str().unwrap(), "Main"),
        expected_stdout("ac_anyconstr"),
        "scalac's own run does not match the recorded output"
    );
    let _ = fs::remove_dir_all(&out);
}

// --------------------------------------- negative: the alias still decides

/// `type G[X] = List[X]` reduces the same way `AnyConstr` does, and its body
/// then refuses: `Option` is not `List`, an abstract `CC` is not `List`
/// either, and the reduction is not symmetric -- an `Ops[Int, AnyConstr,
/// String]` is not an `Ops[Int, CC, String]`. These three already failed
/// before the fix; they are here so the widening arm cannot grow into
/// "any constructor of the right arity conforms to any alias of that arity".
#[test]
fn fixtures_ac_anyconstr_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not available");
        return;
    };
    compile_fails(
        "ac_anyconstr_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "no matching overload for (Ops[Int, Bad$.G, _])Int with arguments (AbstractOps[CC])",
            "ac_anyconstr_bad.scala:26",
            "no matching overload for (Ops[Int, Bad$.G, _])Int with arguments (Ops[Int, Option, String])",
            "ac_anyconstr_bad.scala:30",
            "type mismatch; found: Ops[Int, Bad$.AnyConstr, String]  required: Ops[Int, CC, String]",
            "ac_anyconstr_bad.scala:35",
        ],
    );
}

/// The same three, straight from scalac, at the same lines, so the fixture
/// cannot drift into asserting a restriction nsc does not have.
#[test]
fn scalac_agrees_ac_anyconstr_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-ac-anyconstr-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("ac_anyconstr_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(
        !output.status.success(),
        "scalac accepted ac_anyconstr_bad.scala"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "ac_anyconstr_bad.scala:26: error: type mismatch",
        "found   : AbstractOps[CC]",
        "ac_anyconstr_bad.scala:30: error: type mismatch",
        "found   : Bad.Ops[Int,Option,String]",
        "ac_anyconstr_bad.scala:35: error: type mismatch",
        "required: Bad.Ops[Int,CC,String]",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac's error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
