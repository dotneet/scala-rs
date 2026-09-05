//! Two limits of the class file format that were being truncated in silence.
//!
//! 1. **A branch offset is 16 bits.** Every branch but `goto_w`/`jsr_w` carries
//!    a signed 16-bit offset (JVMS 6.5), so a jump over more than 32767 bytes
//!    has to be re-encoded -- a `goto` as `goto_w`, a conditional as its
//!    inverse over a `goto_w`, which is what nsc emits by way of ASM. We cast
//!    the offset to `i16` instead, so `scala/test/files/run/t10594.scala`
//!    compiled to `ifeq -7611` and died with
//!    `VerifyError: Expecting a stackmap frame at branch target -7611`.
//!
//! 2. **`code_length` must be under 65536** (JVMS 4.7.3). No encoding of a
//!    longer method exists; nsc says `Method too large` and emits nothing for
//!    the class. We wrote the class anyway, with `frames.retain(|off| off <
//!    (len as u16))` throwing away nearly every stack map frame on the way out
//!    -- a file that `javap` reads back happily and no class loader accepts.
//!
//! The sources are generated rather than checked in: the smallest program that
//! reaches either limit is tens of thousands of statements, and a fixture that
//! size would be re-parsed by every `run_fixtures` sweep for no extra signal.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-ms-bigmethod-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `n` calls of `m()`, a hundred to a line.
fn calls(n: usize) -> String {
    let mut s = String::new();
    for chunk in 0..n.div_ceil(100) {
        let here = (n - chunk * 100).min(100);
        s.push_str("      ");
        for _ in 0..here {
            s.push_str("m();");
        }
        s.push('\n');
    }
    s
}

fn compile(out: &Path, jar: Option<&Path>, src: &Path) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    cmd.arg(src);
    cmd.args(["-d", out.to_str().unwrap()]);
    match jar {
        Some(j) => cmd.args(["--scala-library", j.to_str().unwrap()]),
        None => cmd.arg("--no-scala-library"),
    };
    let output = cmd.output().expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

/// Run `Test` with the verifier on and return stdout.
fn run_main(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Test"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Test failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// What nsc 2.13.16 prints for the same source, when it is installed.
fn nsc_stdout(tag: &str, src: &Path, jar: &Path) -> Option<String> {
    let scalac = find_scalac()?;
    let out = tmp_dir(&format!("{tag}-nsc"));
    let status = Command::new(scalac)
        .args(["-d", out.to_str().unwrap()])
        .arg(src)
        .output()
        .expect("scalac");
    assert!(
        status.status.success(),
        "scalac rejected the source: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    Some(run_main(&out, Some(jar)))
}

/// A conditional branch that has to jump over 57 KB of straight-line code:
/// `scala/test/files/run/t10594.scala`, generated so the test states its own
/// shape. nsc widens the same branch (`ifne 20; goto_w 33109`).
#[test]
fn a_branch_over_a_huge_block_still_verifies() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: no scala-library jar");
        return;
    };
    if !java_available() {
        eprintln!("skip: no java");
        return;
    }
    let dir = tmp_dir("branch");
    let src = dir.join("MsBigBranch.scala");
    fs::write(
        &src,
        format!(
            "class C {{\n  var x = 0\n  def m(): Unit = x += 1\n\
             \n  def t(b: Boolean): Unit = {{\n    if (b) {{\n{}    }}\n  }}\n}}\n\
             \nobject Test {{\n  def main(args: Array[String]): Unit = {{\n\
             \x20   val c = new C\n    c.t(false)\n    println(c.x)\n\
             \x20   c.t(true)\n    println(c.x)\n  }}\n}}\n",
            calls(8273)
        ),
    )
    .unwrap();

    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &src);
    assert!(ok, "compile failed: {msgs}");
    let got = run_main(&out, Some(&jar));
    assert_eq!(got, "0\n8273\n");
    if let Some(want) = nsc_stdout("branch", &src, &jar) {
        assert_eq!(got, want, "nsc prints something else");
    }
}

/// The other two directions: a backward `goto` out of range (a loop whose body
/// is huge) and a `lookupswitch` behind a widened branch -- the switch's
/// alignment padding depends on its own offset, so a rewrite that moved it by
/// anything but a multiple of four would break it.
#[test]
fn a_long_loop_and_a_switch_behind_it_still_verify() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: no scala-library jar");
        return;
    };
    if !java_available() {
        eprintln!("skip: no java");
        return;
    }
    let dir = tmp_dir("loop");
    let src = dir.join("MsBigLoop.scala");
    fs::write(
        &src,
        format!(
            "class C {{\n  var x = 0\n  def m(): Unit = x += 1\n\
             \n  def loop(n: Int): Unit = {{\n    var i = 0\n    while (i < n) {{\n\
             {}      i += 1\n    }}\n  }}\n\
             \n  def sel(b: Boolean, k: Int): Int = {{\n    if (b) {{\n{}    }}\n\
             \x20   try {{\n      k match {{\n        case 1 => x + 1\n\
             \x20       case 7 => x + 7\n        case 99 => throw new RuntimeException(\"boom\")\n\
             \x20       case _ => x\n      }}\n    }} catch {{\n\
             \x20     case _: RuntimeException => -1\n    }}\n  }}\n}}\n\
             \nobject Test {{\n  def main(args: Array[String]): Unit = {{\n\
             \x20   val c = new C\n    c.loop(2)\n    println(c.x)\n\
             \x20   println(c.sel(false, 99))\n    println(c.sel(true, 7))\n\
             \x20   println(c.x)\n  }}\n}}\n",
            calls(6000),
            calls(6000)
        ),
    )
    .unwrap();

    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &src);
    assert!(ok, "compile failed: {msgs}");
    let got = run_main(&out, Some(&jar));
    assert_eq!(got, "12000\n-1\n18007\n18000\n");
    if let Some(want) = nsc_stdout("loop", &src, &jar) {
        assert_eq!(got, want, "nsc prints something else");
    }
}

/// Over 64 KB there is nothing to encode. Report it the way nsc does and write
/// no class file, instead of one whose `code_length` a loader rejects.
#[test]
fn a_method_over_64k_is_reported_and_not_written() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: no scala-library jar");
        return;
    };
    let dir = tmp_dir("toolarge");
    let src = dir.join("MsTooLarge.scala");
    fs::write(
        &src,
        format!(
            "class Big {{\n  var x = 0\n  def m(): Unit = x += 1\n\
             \n  def big(): Unit = {{\n{}  }}\n}}\n",
            calls(20000)
        ),
    )
    .unwrap();

    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &src);
    assert!(!ok, "the compile should fail, but it succeeded: {msgs}");
    assert!(
        msgs.contains("Error while emitting Big"),
        "no `Error while emitting`: {msgs}"
    );
    assert!(
        msgs.contains("Method too large: Big.big ()V"),
        "no `Method too large`: {msgs}"
    );
    assert!(
        !out.join("Big.class").exists(),
        "a class file was written for a method that cannot be encoded"
    );
}
