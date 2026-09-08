//! `agent/unqualname`: the two roots behind the standard library's remaining
//! `not found: value` errors. They turned out to be unrelated, and neither is
//! the scope walk the brief expected.
//!
//! **(a) is not a name-resolution bug at all.** `wait` / `notify` /
//! `notifyAll` were missing from `AnyRef` outright, so they failed *qualified*
//! too -- `this.wait()` was `value wait is not a member of D`, and
//! `(o: AnyRef).notifyAll()` was `value notifyAll is not a member of AnyRef`.
//! Unqualified lookup was working; there was nothing for it to find.
//! `crates/typer/src/prelude_anyval2.rs` now declares all five, as nsc does on
//! `scala.AnyRef`. `anyref_sym`'s JVM name is already `java/lang/Object`, so
//! the emitted call is `invokevirtual java/lang/Object.wait…` -- scalac names
//! the *current* class as the owner (`Cell.wait:()V`) and the JVM resolves
//! both to `java.lang.Object.wait`, which is why the two programs run alike.
//!
//! **(b) is a precedence bug, and it was silent.**
//! `Checker::resolve_type_name` mapped `Int`, `String`, `Object` and the rest
//! to their primitive `Type` by the *name*, before any scope lookup. SLS 2
//! puts a definition at level 1 and the `scala._` wildcard every source
//! carries at level 3, so a `trait Int` in an enclosing template hides
//! `scala.Int` -- and the standard library's
//! `scala.collection.generic.BitOperations` is written as
//! `object BitOperations { trait Int { … }; object Int extends Int }`.
//! `object Int extends Int` therefore took its parent to be `scala.Int` and
//! compiled, with no diagnostic anywhere, to `extends java.lang.Integer`.
//! `import BitOperations.Int._` then offered nothing, which is the whole of
//! `TreeSeqMap`'s eleven `not found: value zero / mask / hasMatch /
//! highestOneBit`. Only `javap` showed the parent; that is why
//! `unqname_shadowed_parent_is_the_local_trait` asserts on it rather than on
//! the file compiling.
//!
//! Library set difference (`tests/scalalib_measure.sh`, 538 files): 604 -> 586
//! errors, the 18 removed being exactly these two clusters, and **nothing
//! new**.
//!
//! Fixtures: `unqname.scala`, `unqname_bad.scala`.

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

/// Unique per call: these tests run concurrently and several compile the same
/// fixture, so a shared output directory would let one test's cleanup delete
/// another's class files.
fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-unqname-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Real scalac 2.13.16, when this machine has the checkout the measurement
/// scripts install. Every test that needs it skips loudly without it.
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

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
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
    let mut cmd = Command::new(bin());
    cmd.arg("compile")
        .arg(src)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()]);
    out_of(cmd.output().expect("run scala-rs compile"))
}

fn compile_scalac(sc: &Path, src: &Path, out: &Path, jar: &Path) -> Run {
    let mut cmd = Command::new(sc);
    cmd.args([
        "-classpath",
        jar.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ])
    .arg(src);
    out_of(cmd.output().expect("run scalac"))
}

/// `-Xverify:all`, so a wrong parent or a wrong receiver is a `VerifyError`
/// rather than a silent pass.
fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
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

fn javap(out: &Path, class: &str) -> String {
    let o = Command::new("javap")
        .args(["-c", "-p", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("run javap");
    assert!(
        o.status.success(),
        "javap {class} failed:\n{}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn ok(run: Run, what: &str) {
    assert!(run.ok, "expected {what} to compile:\n{}", run.text);
}

// ---------------------------------------------------------------------------
// The positive fixture, executed.

/// On an unmodified build of the branch point this fixture does not compile:
/// six unresolved monitor calls, and `Bits.viaType` / `Imp.viaImport` reading
/// `zero` off `scala.Int`.
#[test]
fn unqname_runs() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip unqname_runs: jar or java not present");
        return;
    };
    let dir = tmp_dir("run");
    ok(compile_rs(&fixture("unqname"), &dir, &jar), "unqname");
    assert_eq!(run_java(&dir, &jar), expected_stdout("unqname"));
    let _ = fs::remove_dir_all(&dir);
}

/// The expected file is scalac's, checked by producing it again here. Without
/// this, `expected/unqname.txt` is only what this compiler happened to print
/// the day it was written.
#[test]
fn unqname_expected_output_is_scalacs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip unqname_expected_output_is_scalacs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("scalac");
    ok(
        compile_scalac(&sc, &fixture("unqname"), &dir, &jar),
        "unqname under scalac",
    );
    assert_eq!(run_java(&dir, &jar), expected_stdout("unqname"));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Which symbol, not merely "it compiles".

/// Root (b) produced no diagnostic when it was wrong, so a compile is not
/// evidence. `object Int extends Int` and `class Uses extends Int` must
/// implement the *local* `Bits$Int`; before the fix both came out
/// `extends java.lang.Integer`, which loads, verifies and runs.
#[test]
fn unqname_shadowed_parent_is_the_local_trait() {
    let (Some(jar), true) = (scala_library_jar(), javap_available()) else {
        eprintln!("skip unqname_shadowed_parent_is_the_local_trait: jar or javap not present");
        return;
    };
    let dir = tmp_dir("parent");
    ok(compile_rs(&fixture("unqname"), &dir, &jar), "unqname");
    for class in ["Bits$Uses", "Bits$Int$"] {
        let text = javap(&dir, class);
        let head = text.lines().find(|l| l.contains(class)).unwrap_or("");
        assert!(
            head.contains("Bits$Int") && !head.contains("Integer"),
            "{class} must implement the local Bits$Int, not scala.Int's box:\n{head}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The same two classes under real scalac, so the assertion above is pinned to
/// nsc's answer and not to this backend's habits.
#[test]
fn unqname_shadowed_parent_matches_scalac() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), javap_available()) else {
        eprintln!("skip unqname_shadowed_parent_matches_scalac: jar, scalac or javap not present");
        return;
    };
    let dir = tmp_dir("parentsc");
    ok(
        compile_scalac(&sc, &fixture("unqname"), &dir, &jar),
        "unqname under scalac",
    );
    for class in ["Bits$Uses", "Bits$Int$"] {
        let text = javap(&dir, class);
        let head = text.lines().find(|l| l.contains(class)).unwrap_or("");
        assert!(
            head.contains("Bits$Int") && !head.contains("Integer"),
            "scalac itself must implement Bits$Int for {class}:\n{head}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Root (a): all five `java.lang.Object` overloads are reached, with the right
/// descriptors and with `this` as the receiver.
///
/// The owner in the constant pool is the one deliberate difference from
/// scalac. nsc writes `Cell.wait:()V` (the current class); this backend writes
/// `java/lang/Object.wait:()V`, because `AnyRef` is where the member is
/// declared and `anyref_sym`'s JVM name is `java/lang/Object`. JVM method
/// resolution walks the superclass chain, so both name `java.lang.Object.wait`
/// -- `unqname_runs` and `unqname_expected_output_is_scalacs` are what check
/// that they behave alike.
#[test]
fn unqname_monitor_calls_reach_object() {
    let (Some(jar), true) = (scala_library_jar(), javap_available()) else {
        eprintln!("skip unqname_monitor_calls_reach_object: jar or javap not present");
        return;
    };
    let dir = tmp_dir("monitor");
    ok(compile_rs(&fixture("unqname"), &dir, &jar), "unqname");
    let text = javap(&dir, "Cell");
    for desc in [
        "wait:()V",
        "wait:(J)V",
        "wait:(JI)V",
        "notify:()V",
        "notifyAll:()V",
    ] {
        let line = text
            .lines()
            .find(|l| l.contains(desc))
            .unwrap_or_else(|| panic!("no call site for {desc} in Cell:\n{text}"));
        assert!(
            line.contains("invokevirtual"),
            "{desc} must be an invokevirtual:\n{line}"
        );
    }
    // The receiver is `this`, not a captured local and not the monitor
    // temporary: every one of the six call sites is preceded by `aload_0`
    // (with `lconst_1` / `iconst_0` in between for the timed overloads).
    let lines: Vec<&str> = text.lines().collect();
    let mut sites = 0;
    for (i, l) in lines.iter().enumerate() {
        if !(l.contains("invokevirtual") && (l.contains("wait:") || l.contains("notify"))) {
            continue;
        }
        sites += 1;
        let recv = lines[..i]
            .iter()
            .rev()
            .take(3)
            .find(|p| p.contains("aload_0"));
        assert!(
            recv.is_some(),
            "monitor call must be made on `this`:\n{}",
            lines[i.saturating_sub(3)..=i].join("\n")
        );
    }
    assert_eq!(sites, 6, "expected six monitor call sites in Cell:\n{text}");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The rejections both roots have to keep making.

/// Supplying `AnyRef`'s monitor methods must not make any argument list
/// acceptable, and the shadowing has to actually take effect: if `Int` in
/// `object Shadow` still meant `scala.Int`, `x + 1` would compile.
///
/// scalac rejects the same two lines (`found String("soon") required Long`,
/// and `found Int(1) required String`); the wording differs, so the assertion
/// is on the *positions*, which is the part that has to agree.
#[test]
fn unqname_bad_rejected_where_scalac_rejects() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip unqname_bad_rejected_where_scalac_rejects: jar not present");
        return;
    };
    let dir = tmp_dir("bad");
    let run = compile_rs(&fixture("unqname_bad"), &dir, &jar);
    assert!(
        !run.ok,
        "unqname_bad must be rejected, but it compiled:\n{}",
        run.text
    );
    for needle in ["unqname_bad.scala:12", "unqname_bad.scala:17"] {
        assert!(
            run.text.contains(needle),
            "expected a diagnostic at {needle}:\n{}",
            run.text
        );
    }
    if let Some(sc) = scalac() {
        let scdir = tmp_dir("badsc");
        let scrun = compile_scalac(&sc, &fixture("unqname_bad"), &scdir, &jar);
        assert!(
            !scrun.ok,
            "scalac must reject unqname_bad too:\n{}",
            scrun.text
        );
        for needle in ["unqname_bad.scala:12", "unqname_bad.scala:17"] {
            assert!(
                scrun.text.contains(needle),
                "expected scalac to reject at {needle}:\n{}",
                scrun.text
            );
        }
        let _ = fs::remove_dir_all(&scdir);
    }
    let _ = fs::remove_dir_all(&dir);
}
