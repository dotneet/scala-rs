//! `agent/strarrayops`: an implicit conversion inherited from a base class is
//! weaker than one the object declares itself.
//!
//! The brief for this slice read the scala/scala `src/library` measure's
//! `value apply is not a member of String` / `value stepper is not a member of
//! Array[Short]` family as "the conversion to `StringOps` / `ArrayOps` is not
//! supplying these members in this build", and guessed the cause was the
//! *source* `Predef` being compiled in the same run. It is neither.
//!
//! `Predef.augmentString(s).slice(0, 2)` -- the conversion applied by hand --
//! already worked, and so did `s.head`, `s.take`, `a.map` and 29 of the 32
//! members probed. What failed was exactly the members that **both**
//! `StringOps` and `WrappedString` *declare*: `WrappedString` defines `apply`
//! and overrides `slice` and `stepper`, `ArraySeq` overrides `iterator`,
//! `stepper` and `sorted`. Both conversions were applicable, both declared the
//! member, both took a bare `String` / `Array[A]`, so every tie-break in
//! `search_extension` scored them equal and the search returned `None` -- which
//! the caller reports as `value X is not a member of Y`.
//!
//! nsc breaks that tie by *where each conversion is defined* (SLS 6.26.3): the
//! standard library puts `augmentString` and `genericArrayOps` on `Predef` and
//! `wrapString` and `genericWrapArray` on the `LowPriorityImplicits` that
//! `object Predef extends`, and the derived owner wins. The prelude's
//! hand-written `Predef` has no base class to inherit from, so it carried the
//! same fact as a `low_priority` boolean on the two conversions that needed
//! it; nothing computed it for a hierarchy spelled out in source.
//!
//! So the root is **one**, not one for `String` and one for `Array`, and it is
//! not specific to either type or to `--no-scala-library`: plain user code
//! with `object Conv extends LowPrio` had the defect in **both** modes, which
//! is what `tests/fixtures/strarrayops.scala` is. The library instance is
//! invisible in jar mode only because the prelude's boolean covers it there.
//!
//! Fixture prefix: `strarrayops_`, plus `strarrayops.scala`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-strarrayops-{tag}-{}-{nanos}-{seq}",
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
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

struct Run {
    ok: bool,
    text: String,
}

fn out_of(output: std::process::Output) -> Run {
    Run {
        ok: output.status.success(),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn compile_rs(src: &Path, out: &Path, jar: &Path) -> Run {
    out_of(
        Command::new(bin())
            .arg("compile")
            .arg(src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    )
}

fn compile_rs_private(src: &Path, out: &Path) -> Run {
    out_of(
        Command::new(bin())
            .arg("compile")
            .arg(src)
            .args(["-d", out.to_str().unwrap()])
            .arg("--no-scala-library")
            .output()
            .expect("run scala-rs compile"),
    )
}

fn compile_scalac(sc: &Path, src: &Path, out: &Path, jar: &Path) -> Run {
    out_of(
        Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(src)
            .output()
            .expect("run scalac"),
    )
}

/// `-Xverify:all`, so a conversion lowered to a call the receiver does not
/// have is a `VerifyError` rather than a silent pass.
fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn ok(run: Run, what: &str) {
    assert!(run.ok, "expected {what} to compile:\n{}", run.text);
}

// ---------------------------------------------------------------------------
// The positive fixture, executed in both modes.

/// On an unmodified build of the branch point three of these four lines do not
/// compile: `value sel is not a member of "x"`, and the same for `three` and
/// `mix`. `onlyLow` compiled before and after -- only the base declares it, so
/// there was never a tie -- and it is in the fixture because the rule must
/// *demote* an inherited conversion, not remove it.
///
/// It is run, not just compiled, because "which conversion won" is invisible
/// in a compile: both candidates typecheck and both return a `String`. Only
/// the printed word says whether `HighOps#sel` or `LowOps#sel` was called.
#[test]
fn strarrayops_runs_against_the_jar() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip strarrayops_runs_against_the_jar: jar or java not present");
        return;
    };
    let dir = tmp_dir("jar");
    ok(
        compile_rs(&fixture("strarrayops"), &dir, &jar),
        "strarrayops",
    );
    assert_eq!(
        run_java(&dir, Some(&jar)),
        expected_stdout("strarrayops"),
        "the wrong conversion won"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The same file under the private runtime. The `src/library` measurement runs
/// `--no-scala-library`, and the defect was there too -- this mode has no
/// prelude `StringOps` at all, so the `low_priority` boolean that hid it in
/// jar mode is not even installed.
#[test]
fn strarrayops_runs_under_the_private_runtime() {
    if !java_available() {
        eprintln!("skip strarrayops_runs_under_the_private_runtime: java not present");
        return;
    }
    let dir = tmp_dir("priv");
    ok(
        compile_rs_private(&fixture("strarrayops"), &dir),
        "strarrayops under the private runtime",
    );
    assert_eq!(
        run_java(&dir, None),
        expected_stdout("strarrayops"),
        "the wrong conversion won under the private runtime"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The expected file is scalac's, checked by producing it again here. Without
/// this, `expected/strarrayops.txt` is only what this compiler happened to
/// print the day it was written -- and for this slice the whole claim is that
/// the answer matches nsc's, so the check is the point rather than a
/// formality.
#[test]
fn strarrayops_expected_output_is_scalacs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip strarrayops_expected_output_is_scalacs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("scalac");
    ok(
        compile_scalac(&sc, &fixture("strarrayops"), &dir, &jar),
        "strarrayops under scalac",
    );
    assert_eq!(run_java(&dir, Some(&jar)), expected_stdout("strarrayops"));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The library instance the brief was about.

/// `"abcdef".slice(1, 3)`, `"abcdef"(2)`, `"abcdef".stepper` and
/// `Array(3, 1, 2).iterator` are the four selections the `src/library` measure
/// reported. Each is a member that `StringOps`/`ArrayOps` **and**
/// `WrappedString`/`ArraySeq` declare, so each is the tie this slice breaks.
///
/// `Array#stepper` is deliberately not among them, and it is the one place
/// this slice moved an error rather than closing it. Its conversion now
/// resolves, and the call then fails one step further along on
/// `implicit shape: StepperShape[A, S]`, which this compiler cannot solve for
/// an undetermined `S`; the eight `StreamExtensions.scala` sites in the
/// `src/library` measure moved from `value stepper is not a member of
/// Array[Byte]` to `could not find implicit value of type StreamShape[...]`
/// for that reason. `String#stepper` takes no type parameter and does work.
///
/// Executed and compared with scalac's own output, because "compiles" would
/// not distinguish `StringOps#slice` (returns a `String`) from
/// `WrappedString#slice` (returns a `WrappedString`) once both are printed
/// through `toString`-shaped code -- but `sorted` and `iterator` do differ,
/// and a wrong pick shows up as a different line.
#[test]
fn strarrayops_library_selections_agree_with_scalac() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!(
            "skip strarrayops_library_selections_agree_with_scalac: jar, scalac or java absent"
        );
        return;
    };
    let dir = tmp_dir("lib");
    let src = dir.join("Lib.scala");
    fs::write(
        &src,
        r#"object Main {
  val s: String = "abcdef"
  val a: Array[Int] = Array(3, 1, 2)
  def main(args: Array[String]): Unit = {
    println(s.slice(1, 3))
    println(s.apply(2))
    println(s(2))
    println(s.stepper.nextStep())
    println(a.iterator.mkString("-"))
    println(a.sorted.mkString("-"))
    println(s.slice(1, 3).getClass.getName)
    println(a.sorted.getClass.getName)
  }
}
"#,
    )
    .unwrap();
    let mine = dir.join("mine");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&mine).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    ok(
        compile_rs(&src, &mine, &jar),
        "the library-selection fixture",
    );
    ok(
        compile_scalac(&sc, &src, &theirs, &jar),
        "the library-selection fixture under scalac",
    );
    let a = run_java(&mine, Some(&jar));
    let b = run_java(&theirs, Some(&jar));
    assert_eq!(a, b, "scala-rs and scalac disagree on which conversion won");
    // The two `getClass` lines are the ones that would change if the *other*
    // conversion had been picked: `WrappedString#slice` answers a
    // `WrappedString`, `ArraySeq#sorted` an `ArraySeq`.
    assert!(
        a.contains("java.lang.String"),
        "`s.slice(1, 3)` should be `StringOps#slice`, a `String`:\n{a}"
    );
    assert!(
        a.contains("[I"),
        "`a.sorted` should be `ArrayOps#sorted`, an `Array[Int]`:\n{a}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The guard: the rule demotes, it does not silence.

/// Two conversions whose owners are *unrelated* still tie, and a tie is still
/// refused. Without this the fix would read as "when in doubt, pick one".
#[test]
fn strarrayops_unrelated_owners_are_still_ambiguous() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip strarrayops_unrelated_owners_are_still_ambiguous: jar not present");
        return;
    };
    let dir = tmp_dir("bad");
    let run = compile_rs(&fixture("strarrayops_bad"), &dir, &jar);
    assert!(
        !run.ok,
        "`\"x\".sel` with `toP` and `toQ` in unrelated objects must not be accepted:\n{}",
        run.text
    );
    assert!(
        run.text.contains("sel"),
        "the rejection should name the member:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The same file under real scalac. nsc words it as a `type mismatch` with an
/// "implicit conversions ... are ambiguous" note where this compiler says
/// `value sel is not a member`; the wordings are a standing difference, and
/// what is pinned here is that **both** refuse, which is the claim the guard
/// above needs.
#[test]
fn strarrayops_bad_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip strarrayops_bad_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("badsc");
    let run = compile_scalac(&sc, &fixture("strarrayops_bad"), &dir, &jar);
    assert!(
        !run.ok,
        "scalac should refuse strarrayops_bad -- the guard no longer measures an ambiguity:\n{}",
        run.text
    );
    assert!(
        run.text.contains("ambiguous"),
        "scalac should call it ambiguous:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}
