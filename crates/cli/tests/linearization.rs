//! Linearization (SLS 5.1.2): that it *terminates*, and that what it produces
//! is the order real scalac 2.13.16 produces.
//!
//! `crates/typer/src/lin.rs` used to bound its recursion with a depth counter
//! alone. A depth bound bounds the depth of the recursion tree, not its size:
//! with two parents per node on a cycle the tree is `branching^64`, so
//!
//! ```text
//! trait X extends Y with Z
//! trait Y extends Z
//! trait Z extends X
//! ```
//!
//! — three lines real scalac rejects in under two seconds — pinned a core with
//! flat memory for as long as it was left running. Three compiler processes
//! were found in that state after four to five and a half hours.
//!
//! The fix carries the recursion path, so the walk is total on *any* symbol
//! graph. A termination guard is cheap to get wrong in the other direction,
//! though: one that quietly truncated a linearization would leave a
//! well-formed program compiling and change which `super` implementation runs,
//! with no diagnostic at all. So the positive test here does not check that
//! the fixture compiles — it runs it, and compares the printed `super` chain
//! (which *is* the linearization) against the same source compiled and run by
//! real scalac.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
        "scala-rs-lin-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
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

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile `args` and give up after `secs`, rather than letting a
/// non-terminating compiler hang the whole suite until the harness kills it.
/// Returns the wall clock and the combined diagnostics; `None` means the
/// deadline passed and the child was killed.
fn compile_within(args: &[&str], secs: u64) -> Option<(Duration, String)> {
    let out = tmp_dir("within");
    let mut child = Command::new(bin())
        .args(args)
        .args(["-d", out.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn scala-rs");
    let start = Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None if start.elapsed() > Duration::from_secs(secs) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_dir_all(&out);
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let elapsed = start.elapsed();
    let done = child.wait_with_output().expect("wait_with_output");
    let _ = fs::remove_dir_all(&out);
    Some((
        elapsed,
        format!(
            "{}{}",
            String::from_utf8_lossy(&done.stderr),
            String::from_utf8_lossy(&done.stdout)
        ),
    ))
}

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn compile_ours(src: &Path, tag: &str) -> PathBuf {
    let out = tmp_dir(tag);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile {} failed", src.display());
    out
}

/// The printed `super` chain of a deep, wide, diamond-shaped hierarchy is the
/// linearization. Compared against real scalac 2.13.16 running the *same*
/// source, so this is an oracle and not a transcription of our own output.
#[test]
fn super_chain_matches_scalac() {
    if !java_available() {
        eprintln!("skip super_chain_matches_scalac: no java");
        return;
    }
    let src = fixture("linterm_diamond");
    let ours_out = compile_ours(&src, "diamond-ours");
    let ours = run_main(ours_out.to_str().unwrap());

    // Ten trait levels over four diamonds: a truncated linearization drops a
    // name from one of these chains instead of failing to compile.
    assert!(
        ours.contains("Deep L9 L8 L7 L6 L5 L4 L3 L2 L1 L0"),
        "super chain lost a trait: {ours}"
    );
    // The shape SLS 5.1.2's `+:` gets right and a C3 merge does not. It used to
    // compile cleanly and print a *different* chain, so it is named here as
    // well as covered by the whole-output comparison below: a regression should
    // say which shape broke, not just that some line moved.
    //
    // `class Wider extends Root with L6 with L5` reaches `L4` at two
    // different depths (through `L3` from `L5`, directly from `L6`). The fold
    // `L(L5) +: L(L6) +: L(Root)` keeps `L3` immediately behind `L5`; the C3
    // merge, with no free head, fell back to its first list and printed
    // `Wider L5 L6 L4 L3 L1 L2 L0`.
    assert!(
        ours.contains("Wider L5 L3 L6 L4 L1 L2 L0"),
        "a shared ancestor reached at two depths is misplaced: {ours}"
    );

    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip scalac oracle for linterm_diamond: scalac or jar not obtainable");
        let _ = fs::remove_dir_all(&ours_out);
        return;
    };
    let theirs_out = tmp_dir("diamond-scalac");
    let scalac_run = Command::new(&scalac)
        .args([
            "-classpath",
            jar.to_str().unwrap(),
            "-d",
            theirs_out.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output()
        .expect("run scalac");
    assert!(
        scalac_run.status.success(),
        "scalac oracle failed to compile linterm_diamond: {}",
        String::from_utf8_lossy(&scalac_run.stderr)
    );
    let theirs = run_main(&format!("{}:{}", theirs_out.display(), jar.display()));
    assert_eq!(
        theirs, ours,
        "linearization differs from scalac 2.13.16 for linterm_diamond"
    );
    let _ = fs::remove_dir_all(&ours_out);
    let _ = fs::remove_dir_all(&theirs_out);
}

/// A cyclic `extends` graph is rejected, once per cycle, at the line scalac
/// reports and with scalac's own wording — and, above all, it *finishes*.
#[test]
fn cyclic_extends_is_rejected_and_terminates() {
    let src = fixture("linterm_cycle_bad");
    let path = src.to_str().unwrap().to_string();
    // Sixty seconds is four orders of magnitude more than this takes; the
    // point is that a regression fails the test instead of hanging the suite.
    let Some((elapsed, diags)) = compile_within(&["compile", &path, "--no-scala-library"], 60)
    else {
        panic!("compiling linterm_cycle_bad did not terminate within 60s");
    };
    assert!(
        elapsed < Duration::from_secs(60),
        "compiling a cyclic hierarchy took {elapsed:?}"
    );
    for (line, msg) in [
        (15, "illegal cyclic reference involving trait X"),
        (19, "illegal cyclic reference involving class C"),
        (22, "illegal cyclic reference involving trait S"),
    ] {
        assert!(
            diags.contains(msg),
            "expected {msg:?} for linterm_cycle_bad, got:\n{diags}"
        );
        assert!(
            diags.contains(&format!("linterm_cycle_bad.scala:{line}:")),
            "expected the cycle reported at line {line}, got:\n{diags}"
        );
    }
    // One diagnostic per cycle, as scalac emits: three cycles, three errors.
    assert_eq!(
        diags.matches("illegal cyclic reference").count(),
        3,
        "expected exactly one diagnostic per cycle, got:\n{diags}"
    );
}

/// The lines and the wording above are scalac's, not ours. This checks that
/// claim against the real compiler rather than trusting the comment.
#[test]
fn cyclic_extends_matches_scalacs_lines() {
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip scalac oracle for linterm_cycle_bad: scalac or jar not obtainable");
        return;
    };
    let src = fixture("linterm_cycle_bad");
    let out = tmp_dir("cycle-scalac");
    let run = Command::new(&scalac)
        .args([
            "-classpath",
            jar.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output()
        .expect("run scalac");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
    assert!(
        !run.status.success(),
        "scalac accepted a cyclic hierarchy: {said}"
    );
    for expected in [
        "linterm_cycle_bad.scala:15: error: illegal cyclic reference involving trait X",
        "linterm_cycle_bad.scala:19: error: illegal cyclic reference involving class C",
        "linterm_cycle_bad.scala:22: error: illegal cyclic reference involving trait S",
    ] {
        assert!(
            said.contains(expected),
            "scalac 2.13.16 no longer says {expected:?}; it said:\n{said}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// A parent that cannot be resolved must leave the class with an unresolved
/// parent, never with a parent that happens to share the missing type's
/// *simple* name. cats' `trait AllOps extends Ops with Apply.AllOps` with
/// `Apply.scala` absent resolved `Apply.AllOps` to the enclosing `AllOps`
/// itself, which is what turned a missing file into a self-inheriting trait
/// and, before the fix, into a compiler that never returned.
///
/// scalac says `not found: value Missing` here, and so does this compiler now
/// that `Typer::qualifier_names_nothing` narrows that fallback to qualifiers
/// which denote something (the wording is pinned in
/// `crates/cli/tests/qualfallback.rs`). What this test pins is the part that
/// must never regress: the compilation *ends*, and it ends with an error
/// rather than with class files.
#[test]
fn unresolved_qualified_parent_terminates_with_a_diagnostic() {
    let src = fixture("linterm_missing_prefix_bad");
    let path = src.to_str().unwrap().to_string();
    let Some((_, diags)) = compile_within(&["compile", &path, "--no-scala-library"], 60) else {
        panic!("compiling linterm_missing_prefix_bad did not terminate within 60s");
    };
    assert!(
        diags.contains("error"),
        "a parent naming a type that does not exist was accepted: {diags}"
    );
}
