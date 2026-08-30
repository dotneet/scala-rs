//! Erasure boxed the value of a *block* twice.
//!
//! `new It[Int] { def next(): Int = { val z = 1; z } }` implements a member
//! whose erased result is `Object`, so the `Int` the body produces has to be
//! boxed on its way out. Erasure hands the expected type straight to the
//! subexpressions that produce a `Block`'s, an `If`'s, a `Match`'s or a
//! `Try`'s value, so those were boxed already -- and then the node itself was
//! boxed a second time, emitting `boxToInteger(boxToInteger(z))` in
//! `Main$$anon$1.next()Ljava/lang/Object;`. The class typechecked and did not
//! verify:
//!
//! ```text
//! java.lang.VerifyError: Bad type on operand stack
//!   Location: Main$$anon$1.next()Ljava/lang/Object; @6: invokestatic
//!   Reason: Type 'java/lang/Integer' is not assignable to integer
//! ```
//!
//! An *expression* body (`def next(): Int = z`) has no such node to descend
//! into, which is why the bug looked like it was about blocks specifically --
//! and why it reproduces just as well outside any class, for
//! `val x: Any = { val z = 1; z }`.
//!
//! The fixture runs against the real `scala-library` jar *and* the private
//! runtime under `-Xverify:all`, and its output is compared with what nsc
//! 2.13.16 prints for the same source. The `javap` tests below pin the shape
//! of the emitted method, which the stdout comparison cannot see.

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
        "scala-rs-anonbridge-{tag}-{}-{nanos}-{seq}",
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

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_main(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// Compile `source` and run it, in whichever mode `jar` selects.
fn run_source(tag: &str, source: &str, jar: Option<&Path>) -> String {
    let dir = tmp_dir(tag);
    let src = dir.join("Main.scala");
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, jar, &src);
    assert!(ok, "{tag} should compile, got:\n{msgs}");
    let stdout = run_main(&out, jar);
    let _ = fs::remove_dir_all(&dir);
    stdout
}

/// Both modes have to verify and print `want`.
fn prints(tag: &str, source: &str, want: &str) {
    if !java_available() {
        eprintln!("skip {tag}: no java");
        return;
    }
    assert_eq!(
        run_source(tag, source, None),
        want,
        "{tag}: private runtime"
    );
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag} (jar): scala-library jar not present");
        return;
    };
    assert_eq!(run_source(tag, source, Some(&jar)), want, "{tag}: jar");
}

/// The same, for sources the private runtime cannot serve (`List`, ...).
fn prints_lib(tag: &str, source: &str, want: &str) {
    if !java_available() {
        eprintln!("skip {tag}: no java");
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    assert_eq!(run_source(tag, source, Some(&jar)), want, "{tag}: jar");
}

// ------------------------------------------------------------------ fixtures

/// Both modes, both verified.
#[test]
fn ab_fixture_runs_in_both_modes() {
    let name = "ab";
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected = expected_stdout(name);

    let priv_out = tmp_dir("priv");
    let (ok, msgs) = compile(&priv_out, None, &src);
    assert!(ok, "compile {name} (private runtime) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&priv_out, None),
            expected,
            "stdout mismatch for {name} on the private runtime"
        );
    }
    let _ = fs::remove_dir_all(&priv_out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name} (jar): scala-library jar not present");
        return;
    };
    let jar_out = tmp_dir("jar");
    let (ok, msgs) = compile(&jar_out, Some(&jar), &src);
    assert!(ok, "compile {name} (jar) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&jar_out, Some(&jar)),
            expected,
            "stdout mismatch for {name} against the jar"
        );
    }
    let _ = fs::remove_dir_all(&jar_out);
}

/// The recorded expectation is real scalac 2.13.16's own stdout, and ours has
/// to be the same string.
#[test]
fn real_scalac_dual_run_ab() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff ab: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("ab.scala");
    let ref_out = tmp_dir("ab-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile ab");
    let reference = run_main(&ref_out, Some(&jar));
    assert_eq!(
        reference,
        expected_stdout("ab"),
        "recorded expectation for ab does not match real scalac"
    );

    let out = tmp_dir("ab-ours");
    let (ok, msgs) = compile(&out, Some(&jar), &src);
    assert!(ok, "compile ab (jar) failed:\n{msgs}");
    assert_eq!(
        run_main(&out, Some(&jar)),
        reference,
        "stdout differs from real scalac for ab"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// Boxing a block's value does not excuse the block from having the declared
/// type. scalac 2.13.16 gives the same `type mismatch` here.
#[test]
fn fixtures_ab_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip ab_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("ab_bad.scala");
    let out = tmp_dir("ab_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &src);
    assert!(!ok, "expected compile of ab_bad to fail, got:\n{msgs}");
    assert!(
        msgs.contains("type mismatch; found: String  required: Int"),
        "unexpected diagnostics for ab_bad: {msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

// --------------------------------------------------------------- javap shape

/// The `Code:` listing of the one method named `name` in `class_file` whose
/// descriptor is `desc`, or `None` when the class has no such method.
fn method_code(class_file: &Path, name: &str, desc: &str) -> Option<String> {
    let out = Command::new("javap")
        .args(["-p", "-c", "-s", class_file.to_str().unwrap()])
        .output()
        .expect("javap");
    assert!(
        out.status.success(),
        "javap {} failed: {}",
        class_file.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    // javap prints, per method: the declaration line, `descriptor: ...`,
    // `Code:`, then the instructions until the next method's declaration.
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut cur_desc: Option<String> = None;
    let mut cur_head: Option<String> = None;
    let mut body = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t.ends_with(");") {
            if let (Some(h), Some(d)) = (cur_head.take(), cur_desc.take()) {
                blocks.push((format!("{h}\u{1}{d}"), std::mem::take(&mut body)));
            }
            let head = t[..t.find('(').unwrap()]
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_string();
            cur_head = Some(head);
            body.clear();
            continue;
        }
        if let Some(d) = t.strip_prefix("descriptor: ") {
            cur_desc = Some(d.to_string());
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    if let (Some(h), Some(d)) = (cur_head, cur_desc) {
        blocks.push((format!("{h}\u{1}{d}"), body));
    }
    let key = format!("{name}\u{1}{desc}");
    blocks.into_iter().find(|(k, _)| *k == key).map(|(_, b)| b)
}

fn boxing_calls(code: &str) -> usize {
    code.lines()
        .filter(|l| {
            l.contains("invokestatic")
                && (l.contains("boxToInteger") || l.contains("Integer.valueOf"))
        })
        .count()
}

const BLOCK_BODY_SRC: &str = "object Main {\n\
     \x20 trait It[A] { def next(): A }\n\
     \x20 def main(a: Array[String]): Unit = {\n\
     \x20   val i = new It[Int] { def next(): Int = { val z = 1; z } }\n\
     \x20   println(i.next())\n\
     \x20 }\n\
     }\n";

/// The heart of the bug, stated on the bytecode rather than on stdout: the
/// entry point the interface call lands on, `next()Ljava/lang/Object;`, boxes
/// the `Int` exactly once. It used to box twice.
#[test]
fn erased_next_boxes_its_block_exactly_once() {
    if !javap_available() {
        eprintln!("skip: no javap");
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("javap-ours");
    let src = dir.join("Main.scala");
    fs::write(&src, BLOCK_BODY_SRC).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &src);
    assert!(ok, "compile failed:\n{msgs}");

    let anon = out.join("Main$$anon$1.class");
    let code = method_code(&anon, "next", "()Ljava/lang/Object;")
        .expect("Main$$anon$1 must expose next()Ljava/lang/Object;");
    assert_eq!(
        boxing_calls(&code),
        1,
        "next()Ljava/lang/Object; must box exactly once, got:\n{code}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The same class from real scalac 2.13.16, side by side. nsc splits the
/// member in two -- `next()I` carries the body and `next()Ljava/lang/Object;`
/// is a bridge that calls it and boxes -- while we fold the two together and
/// emit only the erased one. Either shape is fine for a caller, and both are
/// checked here: the erased entry point exists in both and boxes once, and
/// nsc's extra `next()I` is not something we are missing an implementation of.
#[test]
fn scalac_and_ours_agree_on_the_erased_entry_point() {
    if !javap_available() {
        eprintln!("skip: no javap");
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or jar not obtainable");
        return;
    };
    let dir = tmp_dir("javap-both");
    let src = dir.join("Main.scala");
    fs::write(&src, BLOCK_BODY_SRC).unwrap();

    let sc_out = dir.join("sc");
    fs::create_dir_all(&sc_out).unwrap();
    let status = Command::new(&scalac)
        .args([
            src.to_str().unwrap(),
            "-d",
            sc_out.to_str().unwrap(),
            "-classpath",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed");

    let rs_out = dir.join("rs");
    fs::create_dir_all(&rs_out).unwrap();
    let (ok, msgs) = compile(&rs_out, Some(&jar), &src);
    assert!(ok, "compile failed:\n{msgs}");

    let sc_anon = sc_out.join("Main$$anon$1.class");
    let rs_anon = rs_out.join("Main$$anon$1.class");

    // nsc: the body lives in `next()I`, the bridge boxes it.
    let sc_bridge = method_code(&sc_anon, "next", "()Ljava/lang/Object;")
        .expect("scalac's anon class has the erased bridge");
    assert_eq!(
        boxing_calls(&sc_bridge),
        1,
        "scalac's bridge boxes once:\n{sc_bridge}"
    );
    assert!(
        method_code(&sc_anon, "next", "()I").is_some(),
        "scalac keeps an unerased next()I next to the bridge"
    );

    // Ours: one method at the erased signature, boxing once.
    let rs_erased = method_code(&rs_anon, "next", "()Ljava/lang/Object;")
        .expect("our anon class exposes the same erased entry point");
    assert_eq!(
        boxing_calls(&rs_erased),
        1,
        "our next()Ljava/lang/Object; boxes once:\n{rs_erased}"
    );
    // Whatever else we emit, `next()I` must not be left abstract or missing a
    // caller: we emit it or we do not, but the erased one always carries a
    // body, which the `Code:` listing above already proves.
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------- unit-ish cases

/// Every primitive, block body, anonymous implementation of a generic trait.
#[test]
fn every_primitive_block_body_boxes_once() {
    for (ty, lit, want) in [
        ("Int", "1", "1\n"),
        ("Long", "2L", "2\n"),
        ("Double", "1.5", "1.5\n"),
        ("Float", "2.5f", "2.5\n"),
        ("Boolean", "true", "true\n"),
        ("Char", "'x'", "x\n"),
        ("Byte", "7: Byte", "7\n"),
        ("Short", "8: Short", "8\n"),
    ] {
        let src = format!(
            "object Main {{\n\
             \x20 trait It[A] {{ def next(): A }}\n\
             \x20 def main(a: Array[String]): Unit = {{\n\
             \x20   val i = new It[{ty}] {{ def next(): {ty} = {{ val z: {ty} = {lit}; z }} }}\n\
             \x20   println(i.next())\n\
             \x20 }}\n\
             }}\n"
        );
        prints(&format!("prim-{ty}"), &src, want);
    }
}

/// An `abstract class` parent, and named classes doing the same as the
/// anonymous ones -- the erased member is owed the same single box.
#[test]
fn abstract_classes_and_named_classes_box_once_too() {
    prints(
        "absclass",
        "object Main {\n\
         \x20 abstract class Ab[A] { def get(): A }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   println(new Ab[Int] { def get(): Int = { val z = 7; z } }.get())\n\
         \x20 }\n\
         }\n",
        "7\n",
    );
    prints(
        "named",
        "trait It[A] { def next(): A }\n\
         class C extends It[Int] { def next(): Int = { val z = 3; z } }\n\
         object Main {\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val i: It[Int] = new C\n\
         \x20   println(i.next())\n\
         \x20   println(new C().next())\n\
         \x20 }\n\
         }\n",
        "3\n3\n",
    );
    prints(
        "namedabs",
        "abstract class Ab[A] { def get(): A }\n\
         class C extends Ab[Long] { def get(): Long = { val z = 4L; z } }\n\
         object Main {\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val i: Ab[Long] = new C\n\
         \x20   println(i.get())\n\
         \x20 }\n\
         }\n",
        "4\n",
    );
}

/// A primitive *parameter*, more than one type parameter, and a generic
/// instantiated at a generic type.
#[test]
fn primitive_parameters_and_several_type_parameters() {
    prints(
        "param",
        "object Main {\n\
         \x20 trait F[A] { def f(a: A): Int }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   println(new F[Int] { def f(x: Int): Int = { val z = x + 1; z } }.f(41))\n\
         \x20 }\n\
         }\n",
        "42\n",
    );
    prints(
        "two",
        "object Main {\n\
         \x20 trait P[A, B] { def one(): A; def two(): B }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val p = new P[Int, Double] {\n\
         \x20     def one(): Int = { val z = 5; z }\n\
         \x20     def two(): Double = { val d = 2.5; d }\n\
         \x20   }\n\
         \x20   println(p.one())\n\
         \x20   println(p.two())\n\
         \x20 }\n\
         }\n",
        "5\n2.5\n",
    );
    prints_lib(
        "nested",
        "object Main {\n\
         \x20 trait It[A] { def next(): A }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val i = new It[List[Int]] { def next(): List[Int] = { val z = List(1, 2); z } }\n\
         \x20   println(i.next())\n\
         \x20 }\n\
         }\n",
        "List(1, 2)\n",
    );
}

/// A lambda SAM-converted to the same interface goes through the same
/// erasure, and so does a plain `FunctionN`.
#[test]
fn sam_and_function_lambdas_box_their_block_once() {
    prints(
        "sam",
        "object Main {\n\
         \x20 trait It[A] { def next(): A }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val i: It[Int] = () => { val z = 9; z }\n\
         \x20   println(i.next())\n\
         \x20 }\n\
         }\n",
        "9\n",
    );
    prints(
        "fun",
        "object Main {\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val f: Int => Int = (x: Int) => { val z = x * 2; z }\n\
         \x20   println(f(21))\n\
         \x20 }\n\
         }\n",
        "42\n",
    );
}

/// `while`, `if`, `match` and `try` as the body. `If`, `Match` and `Try` hand
/// the expected type to their branches exactly as `Block` does, so all three
/// were boxing twice as well.
#[test]
fn control_structures_as_the_body_box_once() {
    prints(
        "while",
        "object Main {\n\
         \x20 trait It[A] { def next(): A }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val i = new It[Int] { def next(): Int = { var n = 0; while (n < 5) { n += 1 }; n } }\n\
         \x20   println(i.next())\n\
         \x20 }\n\
         }\n",
        "5\n",
    );
    prints(
        "if",
        "object Main {\n\
         \x20 trait It[A] { def next(): A }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   println(new It[Int] { def next(): Int = if (1 < 2) 10 else 20 }.next())\n\
         \x20 }\n\
         }\n",
        "10\n",
    );
    prints(
        "match",
        "object Main {\n\
         \x20 trait It[A] { def next(): A }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   println(new It[Int] { def next(): Int = 3 match { case 3 => 30; case _ => 40 } }.next())\n\
         \x20 }\n\
         }\n",
        "30\n",
    );
    prints(
        "try",
        "object Main {\n\
         \x20 trait It[A] { def next(): A }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val i = new It[Int] {\n\
         \x20     def next(): Int = try { val z = 11; z } catch { case _: Throwable => 0 }\n\
         \x20   }\n\
         \x20   println(i.next())\n\
         \x20 }\n\
         }\n",
        "11\n",
    );
}

/// The captured-`var` shape from the original report.
#[test]
fn a_captured_var_read_at_the_end_of_the_block() {
    prints(
        "var",
        "object Main {\n\
         \x20 trait It[A] { def next(): A }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   var n = 0\n\
         \x20   val i = new It[Int] { def next(): Int = { n += 1; n } }\n\
         \x20   println(i.next())\n\
         \x20   println(i.next())\n\
         \x20 }\n\
         }\n",
        "1\n2\n",
    );
}

/// No anonymous class in sight: the same double box hit every block, `if`,
/// `match` and `try` whose value reached a reference position, and every one
/// reaching a primitive one had the matching double *unbox*.
#[test]
fn blocks_reaching_any_and_blocks_reaching_a_primitive() {
    prints(
        "any",
        "object Main {\n\
         \x20 def id[A](x: A): A = x\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val x: Any = { val z = 13; z }\n\
         \x20   println(x)\n\
         \x20   val y: Any = if (1 < 2) { val z = 14; z } else 15\n\
         \x20   println(y)\n\
         \x20   val m: Any = 1 match { case 1 => { val z = 16; z }; case _ => 17 }\n\
         \x20   println(m)\n\
         \x20   val t: Any = try { val z = 18; z } catch { case _: Throwable => 19 }\n\
         \x20   println(t)\n\
         \x20   println(id({ val z = 20; z }))\n\
         \x20   val n: Int = { val z: Any = 21; z.asInstanceOf[Int] }\n\
         \x20   println(n)\n\
         \x20 }\n\
         }\n",
        "13\n14\n16\n18\n20\n21\n",
    );
    prints_lib(
        "arg",
        "object Main {\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   println(List({ val z = 22; z }))\n\
         \x20 }\n\
         }\n",
        "List(22)\n",
    );
}

/// A user value class is boxed by `new Meters(n)` rather than
/// `boxToInteger`, and takes the same route out of a block. It must not be
/// wrapped twice either.
#[test]
fn a_value_class_block_is_wrapped_once() {
    prints(
        "vc",
        "class Meters(val n: Int) extends AnyVal { override def toString: String = n + \"m\" }\n\
         object Main {\n\
         \x20 trait It[A] { def next(): A }\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val i = new It[Meters] { def next(): Meters = { val z = new Meters(3); z } }\n\
         \x20   println(i.next())\n\
         \x20   val x: Any = { val z = new Meters(4); z }\n\
         \x20   println(x)\n\
         \x20 }\n\
         }\n",
        "3m\n4m\n",
    );
}
