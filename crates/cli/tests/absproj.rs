//! E2E tests for the `agent/absproj` slice: a projection out of an *abstract*
//! type (`E#TableElementType`, `B#Session`) that has to stay unreduced until
//! the prefix is instantiated, and reduce then.
//!
//! `slick.lifted.TableQuery[E <: AbstractTable[_]] extends Query[E,
//! E#TableElementType, Seq]` is the shape. The projection was dropped in
//! favour of `TableElementType`'s own *declaration* -- which `AbstractTable`
//! leaves deferred -- so `TableQuery[Accounts]` was a `Query[Accounts, Any,
//! Seq]` and every use downstream had `Any` in it. It is represented the way
//! `agent/projection` represents a path-dependent member, as a deferred
//! `TypeMember` symbol allocated per (prefix, declaration) pair rather than
//! as a new `Type` variant, and it reduces in
//! `SymbolTable::subst_projections` -- once, where the class's type
//! parameters meet their arguments.
//!
//! The negative half is the load-bearing half: reducing `E#ElementType`
//! through `E`'s *bound* while `E` is still abstract would make every
//! `RowSet[E]` a `Rows[E, Any]`, which real scalac 2.13.16 rejects.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with other
//! agents; see `.agent-brief.md`. All fixtures use the `absproj` prefix.

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
        "scala-rs-absproj-{tag}-{}-{nanos}-{seq}",
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
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
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
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(e) => format!("{}:{}", out.display(), e),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Every error `scala-rs` reports for `name`, as `(line, text)`.
fn compile_errors(name: &str, extra: &[&str]) -> Vec<(u32, String)> {
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
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    let mut errs = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("error: ") {
            pending = Some(rest.trim().to_string());
            continue;
        }
        if let (Some(msg), Some(pos)) = (pending.clone(), line.trim_start().strip_prefix("--> ")) {
            // `--> <path>:<line>:<col>`
            let mut parts = pos.rsplitn(3, ':');
            let _col = parts.next();
            if let Some(l) = parts.next().and_then(|n| n.parse::<u32>().ok()) {
                errs.push((l, msg));
            }
            pending = None;
        }
    }
    errs
}

/// The positive fixture executes and prints what real scalac's build prints.
#[test]
fn fixtures_absproj_reduce_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run absproj_reduce: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("absproj_reduce", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("absproj_reduce"),
        "stdout mismatch for library-ABI absproj_reduce"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac 2.13.16: the recorded expectation,
/// scalac's stdout and ours all have to agree.
#[test]
fn real_scalac_dual_run_absproj_reduce() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff absproj_reduce: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("absproj_reduce.scala");
    let ref_out = tmp_dir("absproj_reduce-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(
        status.success(),
        "real scalac failed to compile absproj_reduce"
    );
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout("absproj_reduce"),
        "recorded expectation for absproj_reduce does not match real scalac"
    );
    let out = compile_fixture_with("absproj_reduce", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        reference,
        "stdout differs from real scalac for absproj_reduce"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// The reduction fires where the prefix is known, and *only* there: both
/// rejections are at the lines real scalac 2.13.16 rejects.
///
/// Line 28 is the reduction having really happened (`(String, Int)`, not the
/// deferred declaration); line 32 is the too-eager reduction that would read
/// `E#ElementType` through `E`'s bound and make it `Any`. Slick's
/// `TableQuery` is exactly line 32's shape, and getting it wrong makes
/// unrelated element types conform rather than reporting anything.
#[test]
fn fixtures_absproj_reduce_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let errs = compile_errors(
        "absproj_reduce_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    let lines: Vec<u32> = errs.iter().map(|(l, _)| *l).collect();
    assert_eq!(lines, vec![28, 32], "errors were: {errs:?}");
    assert!(
        errs[0].1.contains("required: Rows[Account, String]"),
        "line 28: {:?}",
        errs[0].1
    );
    assert!(
        errs[1].1.contains("required: Rows[E, Any]"),
        "line 32: {:?}",
        errs[1].1
    );
}

/// An implementation has to meet the *reduced* member type. Kept in its own
/// fixture because scalac's override check runs after the typer and never
/// reports when the typer already has.
#[test]
fn fixtures_absproj_override_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let errs = compile_errors(
        "absproj_override_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        errs.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
        vec![20],
        "errors were: {errs:?}"
    );
    assert!(
        errs[0].1.contains("incompatible type in overriding"),
        "line 20: {:?}",
        errs[0].1
    );
}
