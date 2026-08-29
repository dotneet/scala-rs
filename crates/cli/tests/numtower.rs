//! The numeric conversion tower, and `Byte` / `Short` as JVM primitives.
//!
//! `numt.scala` is dual-run against real scalac 2.13.16 (see
//! `tests/fixtures/expected/numt.txt`, which is scalac's own stdout) in both
//! the private-runtime and the `--scala-library` modes, under
//! `java -Xverify:all` so a wrong instruction sequence fails the verifier
//! rather than silently producing the wrong number.

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
    // Two tests can share a tag, and the clock is not fine enough to
    // separate them: they ran in the same directory and each `java Main` saw
    // the other's half-written output.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-numt-{tag}-{}-{nanos}-{seq}",
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
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// `java -Xverify:all -cp <cp> Main`, asserting the run succeeds.
fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn expect_private_runtime(name: &str) {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    assert_eq!(
        run_main(out.to_str().unwrap()),
        expected_stdout(name),
        "private-runtime stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn expect_library_abi(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip library dual-run {name}: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    let cp = format!("{}:{}", out.display(), jar.display());
    assert_eq!(
        run_main(&cp),
        expected_stdout(name),
        "library-abi stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn diagnostics(name: &str, extra: &[&str]) -> String {
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
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    err
}

/// Compile a throwaway source and return the diagnostics (empty when it built).
fn compile_source(src: &str, extra: &[&str]) -> String {
    let dir = tmp_dir("src");
    let f = dir.join("Main.scala");
    fs::write(&f, src).unwrap();
    let out = dir.join("out");
    let mut cmd = Command::new(bin());
    cmd.args(["compile", f.to_str().unwrap(), "-d", out.to_str().unwrap()]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    if output.status.success() {
        let _ = fs::remove_dir_all(&dir);
        return String::new();
    }
    let _ = fs::remove_dir_all(&dir);
    err
}

#[test]
fn numt_private_runtime_matches_scalac() {
    expect_private_runtime("numt");
}

#[test]
fn numt_library_abi_matches_scalac() {
    expect_library_abi("numt");
}

/// Every one of the 49 `toX` conversions must exist on every numeric class;
/// this is the compile-only shape of `numt.scala`'s first block, so a missing
/// declaration is reported here as "value toX is not a member of ...".
#[test]
fn all_forty_nine_conversions_are_members() {
    let mut body = String::from("object Main {\n  def main(args: Array[String]): Unit = {\n");
    for (i, (ty, lit)) in [
        ("Byte", "1.toByte"),
        ("Short", "1.toShort"),
        ("Char", "'a'"),
        ("Int", "1"),
        ("Long", "1L"),
        ("Float", "1.0f"),
        ("Double", "1.0"),
    ]
    .iter()
    .enumerate()
    {
        body.push_str(&format!("    val v{i}: {ty} = {lit}\n"));
        for to in ["Byte", "Short", "Char", "Int", "Long", "Float", "Double"] {
            body.push_str(&format!("    val r{i}_{to}: {to} = v{i}.to{to}\n"));
        }
    }
    body.push_str("    println(r0_Int)\n  }\n}\n");
    let err = compile_source(&body, &["--no-scala-library"]);
    assert!(
        err.is_empty(),
        "the 7x7 conversion tower must compile: {err}"
    );
}

/// The regression the tower was blocked on: `scala.Byte` is not a class on the
/// JVM, so a member call on a `Byte` parameter used to be emitted as
/// `invokevirtual scala/Byte.toInt` and rejected by the verifier.
#[test]
fn byte_and_short_are_jvm_primitives() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  def take(x: Byte): Int = x.toInt
  def takeS(x: Short): Long = x.toLong
  def give(i: Int): Byte = i.toByte
  def main(args: Array[String]): Unit = {
    println(take(give(200)))
    println(takeS(40000.toShort))
  }
}
"#;
    let dir = tmp_dir("prim");
    let f = dir.join("Main.scala");
    fs::write(&f, src).unwrap();
    let out = dir.join("out");
    let status = Command::new(bin())
        .args([
            "compile",
            f.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "Byte/Short primitives must compile");
    assert_eq!(run_main(out.to_str().unwrap()), "-56\n-25536\n");
    let _ = fs::remove_dir_all(&dir);
}

/// `scala/Byte` and `scala/Short` must not appear as an owner in the constant
/// pool of anything we emit: no such class exists at runtime.
#[test]
fn no_scala_byte_or_short_class_reference() {
    let out = compile_fixture_with("numt", &["--no-scala-library"]);
    for entry in fs::read_dir(&out).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) != Some("class") {
            continue;
        }
        let bytes = fs::read(&p).unwrap();
        for needle in [b"scala/Byte".as_slice(), b"scala/Short".as_slice()] {
            assert!(
                !contains(&bytes, needle),
                "{} references {}",
                p.display(),
                String::from_utf8_lossy(needle)
            );
        }
    }
    let _ = fs::remove_dir_all(&out);
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// The conversions scalac rejects must still be diagnosed, not quietly
/// accepted: implicit narrowing, an out-of-range constant, and `toX` on a
/// `Boolean` or a `Unit`.
#[test]
fn numt_bad_is_diagnosed() {
    let err = diagnostics("numt_bad", &["--no-scala-library"]);
    for needle in [
        "type mismatch; found: Int  required: Byte",
        "type mismatch; found: 300  required: Byte",
        "value toInt is not a member of",
        "value toByte is not a member of",
        "type mismatch; found: Int  required: Char",
    ] {
        assert!(err.contains(needle), "expected {needle:?} in {err:?}");
    }
}

#[test]
fn numt_bad_is_diagnosed_with_library() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip numt_bad library run: jar not obtainable");
        return;
    };
    let err = diagnostics("numt_bad", &["--scala-library", jar.to_str().unwrap()]);
    assert!(
        err.contains("type mismatch; found: Int  required: Byte"),
        "{err}"
    );
}

/// `Array[Byte]` / `Array[Short]` / `Array[Char]` / `Array[Long]` /
/// `Array[Float]` / `Array[Double]` / `Array[Boolean]` each need their own
/// load/store opcode; `aaload` on a `[B` is a `VerifyError`.
#[test]
fn primitive_array_element_opcodes() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val ab = new Array[Byte](1); ab(0) = (-3).toByte; println(ab(0))
    val as = new Array[Short](1); as(0) = (-300).toShort; println(as(0))
    val ac = new Array[Char](1); ac(0) = 'q'; println(ac(0).toInt)
    val al = new Array[Long](1); al(0) = 5L; println(al(0))
    val af = new Array[Float](1); af(0) = 1.5f; println(af(0))
    val ad = new Array[Double](1); ad(0) = 2.5; println(ad(0))
    val az = new Array[Boolean](1); az(0) = true; println(az(0))
  }
}
"#;
    let dir = tmp_dir("arr");
    let f = dir.join("Main.scala");
    fs::write(&f, src).unwrap();
    let out = dir.join("out");
    let status = Command::new(bin())
        .args([
            "compile",
            f.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "primitive arrays must compile");
    assert_eq!(
        run_main(out.to_str().unwrap()),
        "-3\n-300\n113\n5\n1.5\n2.5\ntrue\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Weak conformance (SLS 3.5.3): `Byte <= Short <= Int <= Long <= Float <=
/// Double` and `Char <= Int`, with the widening instruction actually emitted.
#[test]
fn weak_conformance_widens() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val b: Byte = (-1).toByte
    val s: Short = (-1).toShort
    val c: Char = 'a'
    val i: Int = b
    val l: Long = b
    val f: Float = s
    val d: Double = c
    val sh: Short = b
    val lf: Float = 3L
    println(i + " " + l + " " + f + " " + d + " " + sh + " " + lf)
  }
}
"#;
    let dir = tmp_dir("weak");
    let f = dir.join("Main.scala");
    fs::write(&f, src).unwrap();
    let out = dir.join("out");
    let status = Command::new(bin())
        .args([
            "compile",
            f.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "weak conformance must compile");
    assert_eq!(run_main(out.to_str().unwrap()), "-1 -1 -1.0 97.0 -1 3.0\n");
    let _ = fs::remove_dir_all(&dir);
}

/// `1 + 2.5f` used to push an `int` where the verifier wanted a `float`.
#[test]
fn mixed_float_arithmetic_widens() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val i = 1
    val l = 2L
    val f = 2.5f
    println((i + f) + " " + (l + f) + " " + (f + i) + " " + (f + l) + " " + (i < f) + " " + (l < f))
  }
}
"#;
    let dir = tmp_dir("mixf");
    let f = dir.join("Main.scala");
    fs::write(&f, src).unwrap();
    let out = dir.join("out");
    let status = Command::new(bin())
        .args([
            "compile",
            f.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "mixed Int/Float arithmetic must compile");
    assert_eq!(
        run_main(out.to_str().unwrap()),
        "3.5 4.5 3.5 4.5 true true\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// `Ordering[Byte]` / `Ordering[Short]` / `Numeric[Byte]` / `Numeric[Short]`
/// exist now that the two are real types (library ABI only -- the private
/// runtime has no `scala/math/Ordering$Byte$`).
#[test]
fn byte_and_short_have_ordering_and_numeric() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip Ordering[Byte] check: jar not obtainable");
        return;
    };
    let src = r#"
case class P(k: Short, b: Byte)
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(P(2.toShort, 3.toByte), P(1.toShort, 1.toByte))
    println(xs.sortBy(_.k).map(_.k))
    println(xs.sortBy(_.b).map(_.b))
    println(xs.map(_.b).sum)
  }
}
"#;
    let err = compile_source(src, &["--scala-library", jar.to_str().unwrap()]);
    assert!(err.is_empty(), "Ordering/Numeric for Byte and Short: {err}");
}

/// A stable-id `Int` constant pattern against a `Byte`/`Short`/`Char`
/// scrutinee: all three are `int` on the stack and nsc compares them with
/// `==`, so demanding conformance would be wrong.
#[test]
fn int_constant_pattern_against_narrow_scrutinee() {
    let src = r#"
object K { final val ONE = 1 }
object Main {
  def f(x: Short): String = x match { case K.ONE => "one"; case _ => "other" }
  def g(x: Byte): String = x match { case K.ONE => "one"; case _ => "other" }
  def h(x: Char): String = x match { case K.ONE => "one"; case _ => "other" }
  def main(args: Array[String]): Unit = println(f(1.toShort) + g(2.toByte) + h('a'))
}
"#;
    let err = compile_source(src, &["--no-scala-library"]);
    assert!(
        err.is_empty(),
        "constant pattern on a narrow scrutinee: {err}"
    );
}

/// The emitted classfiles must not be readable only by a lenient verifier.
#[test]
fn numt_classfiles_pass_split_verifier() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with("numt", &["--no-scala-library"]);
    let output = Command::new("java")
        .args([
            "-Xverify:all",
            "-XX:+UnlockDiagnosticVMOptions",
            "-cp",
            out.to_str().unwrap(),
            "Main",
        ])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "verification failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&out);
}

/// Sanity: the fixture's expected output really is scalac's, i.e. it has the
/// 7 conversion rows for each source type and did not get truncated.
#[test]
fn numt_expected_covers_every_source_type() {
    let txt = expected_stdout("numt");
    for tag in ["B ", "S ", "C ", "I ", "J ", "F ", "D "] {
        assert!(
            txt.lines().any(|l| l.starts_with(tag)),
            "expected/numt.txt has no {tag:?} row"
        );
    }
}
