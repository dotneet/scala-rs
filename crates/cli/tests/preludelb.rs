//! `agent/preludelb`: the `[B >: A]` members `agent/lowerbound` left named.
//!
//! `agent/lowerbound` fixed `List`'s `sorted` / `min` / `max` / `sum` /
//! `product` and enumerated the rest with a `Sup`/`Sub` probe. This slice
//! re-ran that probe over `List`, `Map`, `Set` and `Option` -- 71 calls, each
//! compiled by this compiler and by real scalac 2.13.16 and compared on
//! accept/reject in **both** directions -- and closed what it found:
//!
//! | member | was | is |
//! |---|---|---|
//! | `contains` | `(A): Boolean` | `[A1 >: A](A1): Boolean` |
//! | `indexOf` | `(A): Int` | `[B >: A](B): Int`, plus the `(B, Int)` arity |
//! | `reduce` | `((A, A) => A): A` | `[B >: A]((B, B) => B): B` |
//! | `reduceLeft` | `((A, A) => A): A` | `[B >: A]((B, A) => B): B` |
//! | `reduceRight` | `((A, A) => A): A` | `[B >: A]((A, B) => B): B` |
//! | `toArray` | `(implicit ClassTag[A]): Array[A]` | `[B >: A](implicit ClassTag[B]): Array[B]` |
//! | `Map.+` | `((K, V)): Map[K, V]` | `[V1 >: V]((K, V1)): Map[K, V1]` |
//! | `Map.updated` | `(Any, Any): Map[K, V]` | `[V1 >: V](Any, V1): Map[K, V1]` |
//!
//! The last two of those the brief did not name. `toArray` was a sixth member
//! of the `sorted` family's exact shape; `Map.updated` was worse than the
//! others, because it *accepted* the widening call and silently answered at
//! the un-widened type -- `Map` is covariant in `V`, so an ascription to
//! `Map[K, Animal]` cannot tell the two apart, and `preludelb_bad` is the
//! narrow ascription that can.
//!
//! **Why this suite executes.** The reason `agent/lowerbound` stopped here is
//! that `sum` moving from `A` to `B` needed its erasure re-checked, and
//! `reduce` / `reduceLeft` take a *function* of the widened type, so the
//! question is sharper for them. Measured with `javap -c`: the descriptor is
//! `(Lscala/Function2;)Ljava/lang/Object;` before and after and under scalac,
//! the emitted code for `List(1,2,3).reduce(_ + _)` is **byte-identical** to
//! the branch point's, and against scalac the boxing and unboxing fall at the
//! same points (this backend spells them `Integer.valueOf` /
//! `checkcast; intValue` where nsc spells them `BoxesRunTime.boxToInteger` /
//! `unboxToInt`, which is its standing strategy on untouched members such as
//! `head` and `foldLeft`, not anything this slice introduced). So the fixture
//! is run, not just compiled, and at three element types: primitive, widened
//! by an argument, and widened past everything by `[Any]`.
//!
//! Fixture prefix: `preludelb_`, plus `preludelb.scala`.

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
        "scala-rs-preludelb-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`, so a wrong inferred element type is a `VerifyError` rather
/// than a silent pass.
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

fn ok(run: Run, what: &str) {
    assert!(run.ok, "expected {what} to compile:\n{}", run.text);
}

fn rejected(run: Run, what: &str, needles: &[&str]) {
    assert!(
        !run.ok,
        "expected {what} to be rejected, but it compiled:\n{}",
        run.text
    );
    for n in needles {
        assert!(
            run.text.contains(n),
            "expected {what}'s rejection to mention {n:?}:\n{}",
            run.text
        );
    }
}

// ---------------------------------------------------------------------------
// The positive fixture, executed.

/// Every widened member, run. On an unmodified build of the branch point this
/// fixture does not compile: `contains`, `indexOf`, all three `reduce`s,
/// `toArray(ClassTag[Animal])` and `Map.+` are each rejected.
#[test]
fn preludelb_runs() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip preludelb_runs: jar or java not present");
        return;
    };
    let dir = tmp_dir("run");
    ok(compile_rs(&fixture("preludelb"), &dir, &jar), "preludelb");
    assert_eq!(run_java(&dir, &jar), expected_stdout("preludelb"));
    let _ = fs::remove_dir_all(&dir);
}

/// The expected file is scalac's, checked by producing it again here. Without
/// this, `expected/preludelb.txt` is only what this compiler happened to
/// print the day it was written.
#[test]
fn preludelb_expected_output_is_scalacs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip preludelb_expected_output_is_scalacs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("scalac");
    ok(
        compile_scalac(&sc, &fixture("preludelb"), &dir, &jar),
        "preludelb under scalac",
    );
    assert_eq!(run_java(&dir, &jar), expected_stdout("preludelb"));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Erasure. The reason `agent/lowerbound` stopped short of these members.

/// `reduce` / `reduceLeft` / `reduceRight` / `contains` / `indexOf` /
/// `toArray` must erase exactly as scalac erases them: the widened parameter
/// is a *function* of `B` for the first three, which is the shape where an
/// erasure mistake would not be caught by the descriptor of a plain value
/// parameter.
///
/// The two spellings this backend uses for boxing (`Integer.valueOf` and
/// `checkcast; intValue` against nsc's `BoxesRunTime.boxToInteger` and
/// `unboxToInt`) are its standing strategy on members this slice does not
/// touch, so what is asserted is the **descriptor** and the **presence** of a
/// box or unbox, not the helper's name.
#[test]
fn preludelb_erases_and_unboxes_where_scalac_does() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), javap_available()) else {
        eprintln!(
            "skip preludelb_erases_and_unboxes_where_scalac_does: jar, scalac or javap absent"
        );
        return;
    };
    let dir = tmp_dir("erase");
    let src = dir.join("Erase.scala");
    fs::write(
        &src,
        r#"object Main {
  val ints: List[Int] = List(1, 2, 3)
  def r1: Int = ints.reduce(_ + _)
  def r2: Int = ints.reduceLeft(_ + _)
  def r3: Int = ints.reduceRight(_ + _)
  def w: Any = ints.reduce[Any]((a, b) => a)
  def c1: Boolean = ints.contains(2)
  def c2: Int = ints.indexOf(2)
  def t: Array[Int] = ints.toArray
  def main(args: Array[String]): Unit = println("" + r1 + r2 + r3 + w + c1 + c2 + t.length)
}
"#,
    )
    .unwrap();
    let mine = dir.join("mine");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&mine).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    ok(compile_rs(&src, &mine, &jar), "the erasure fixture");
    ok(
        compile_scalac(&sc, &src, &theirs, &jar),
        "the erasure fixture under scalac",
    );

    let a = javap(&mine);
    let b = javap(&theirs);

    // The descriptors the widened members are called at. `B` erases to
    // `Object` exactly as `A` did, so none of these may change.
    for d in [
        "reduce:(Lscala/Function2;)Ljava/lang/Object;",
        "reduceLeft:(Lscala/Function2;)Ljava/lang/Object;",
        "reduceRight:(Lscala/Function2;)Ljava/lang/Object;",
        "contains:(Ljava/lang/Object;)Z",
        "indexOf:(Ljava/lang/Object;)I",
        "toArray:(Lscala/reflect/ClassTag;)Ljava/lang/Object;",
    ] {
        assert!(a.contains(d), "scala-rs should call {d}:\n{a}");
        assert!(
            b.contains(d),
            "scalac should call {d} too -- the fixture no longer measures what it claims:\n{b}"
        );
    }

    // How many times each *call site* boxes an argument and unboxes a result.
    // Per method, not over the whole class: the `$anonfun$` bodies differ for
    // an unrelated and already-recorded reason -- nsc's `specialize` gives the
    // lambda an `apply$mcIII$sp` taking two `int`s, and this compiler emits a
    // plain `Function2` whose body unboxes. That gap is `docs/specialization.md`'s
    // and is visible on untouched members (`foldLeft` shows it too); counting
    // it here would measure it instead of the bound.
    //
    // Expected, and identical on both sides: `r1`/`r2`/`r3`/`c2` unbox an
    // `Int` result once each, `c1`/`c2` box the `Int` argument once each, and
    // `w` -- the call widened to `[Any]` -- does neither.
    for m in ["r1", "r2", "r3", "w", "c1", "c2", "t"] {
        let (am, bm) = (body(&a, m), body(&b, m));
        assert_eq!(
            unboxes(&am),
            unboxes(&bm),
            "`{m}` unboxes {} times here and {} times under scalac\n--- scala-rs\n{am}\n--- scalac\n{bm}",
            unboxes(&am),
            unboxes(&bm)
        );
        assert_eq!(
            boxes(&am),
            boxes(&bm),
            "`{m}` boxes {} times here and {} times under scalac\n--- scala-rs\n{am}\n--- scalac\n{bm}",
            boxes(&am),
            boxes(&bm)
        );
    }
    assert_eq!(
        unboxes(&body(&a, "w")),
        0,
        "the `[Any]` call must not unbox"
    );
    assert_eq!(
        unboxes(&body(&a, "r1")),
        1,
        "`reduce` on a `List[Int]` must unbox its result exactly once"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn javap(dir: &Path) -> String {
    let o = Command::new("javap")
        .args(["-p", "-c", "-cp", dir.to_str().unwrap(), "Main$"])
        .output()
        .expect("run javap");
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The disassembly of one method: from its declaration line to the blank line
/// that ends its `Code:` block.
fn body(javap_out: &str, name: &str) -> String {
    let head = format!(" {name}();");
    let mut lines = javap_out.lines().skip_while(|l| !l.ends_with(&head));
    let decl = lines
        .next()
        .unwrap_or_else(|| panic!("no method `{name}` in:\n{javap_out}"));
    let mut out = String::from(decl);
    for l in lines {
        if l.trim().is_empty() {
            break;
        }
        out.push('\n');
        out.push_str(l);
    }
    out
}

/// `checkcast java/lang/Integer; invokevirtual intValue` here,
/// `BoxesRunTime.unboxToInt` under nsc.
fn unboxes(text: &str) -> usize {
    text.matches("Integer.intValue").count() + text.matches("BoxesRunTime.unboxToInt").count()
}

fn boxes(text: &str) -> usize {
    text.matches("Integer.valueOf").count() + text.matches("BoxesRunTime.boxToInteger").count()
}

// ---------------------------------------------------------------------------
// Negatives. Each is paired with the same file under real scalac, so the
// sentence being asserted is nsc's and not ours.

/// The one the branch point **accepted, with no diagnostic**: `updated` was
/// `(Any, Any): Map[K, V]`, so it took the widening argument and handed back
/// the receiver's own value type. Nothing weaker than a narrow ascription
/// catches it -- `Map` is covariant in `V`.
#[test]
fn preludelb_unwidened_map_result_is_reported() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip preludelb_unwidened_map_result_is_reported: jar not present");
        return;
    };
    let dir = tmp_dir("bad");
    rejected(
        compile_rs(&fixture("preludelb_bad"), &dir, &jar),
        "`md.updated(k, cat)` read back at the receiver's own value type",
        &["found: Animal", "required: Dog"],
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn preludelb_bad_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip preludelb_bad_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("badsc");
    rejected(
        compile_scalac(&sc, &fixture("preludelb_bad"), &dir, &jar),
        "preludelb_bad under scalac",
        &["found   : Animal", "required: Dog"],
    );
    let _ = fs::remove_dir_all(&dir);
}

/// `toArray`'s uninstantiated `B` must not reach the message. Both compilers
/// refuse this file; the branch point refused it saying `found: Array[B]`,
/// naming a parameter of a signature the programmer never wrote.
#[test]
fn preludelb_toarray_does_not_report_an_uninstantiated_parameter() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip preludelb_toarray_does_not_report_an_uninstantiated_parameter: jar absent");
        return;
    };
    let dir = tmp_dir("bad2");
    let run = compile_rs(&fixture("preludelb_bad2"), &dir, &jar);
    rejected(
        run,
        "`dogs.toArray(ctAnimal)` ascribed to `Array[Dog]`",
        &["found: Array[Animal]", "required: Array[Dog]"],
    );
    let again = compile_rs(&fixture("preludelb_bad2"), &dir, &jar);
    assert!(
        !again.text.contains("Array[B]"),
        "the message still names an uninstantiated `B`:\n{}",
        again.text
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn preludelb_bad2_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip preludelb_bad2_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("bad2sc");
    // nsc minimises `B` over the expected type and so reports the argument
    // rather than the result; the fixture says why.
    rejected(
        compile_scalac(&sc, &fixture("preludelb_bad2"), &dir, &jar),
        "preludelb_bad2 under scalac",
        &["ClassTag[Animal]", "ClassTag[Dog]"],
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The bound is still a bound and the operands are still not
/// interchangeable: exactly two of the three lines are refused, and the
/// middle one -- `reduceLeft` with a *widened element operand*, which is
/// legal because both operands are contravariant -- is not.
#[test]
fn preludelb_bound_violations_are_still_refused() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip preludelb_bound_violations_are_still_refused: jar not present");
        return;
    };
    let dir = tmp_dir("bad3");
    let run = compile_rs(&fixture("preludelb_bad3"), &dir, &jar);
    rejected(
        run,
        "a `reduce` below the bound and a `reduce` narrowed on the way out",
        &[
            "found: (Cat, Cat) => Cat",
            "required: (Animal, Animal) => Animal",
            "found: Animal",
            "required: Dog",
        ],
    );
    let again = compile_rs(&fixture("preludelb_bad3"), &dir, &jar);
    assert_eq!(
        again.text.matches("error:").count(),
        2,
        "`val b` widens the element operand of `reduceLeft`, which is legal:\n{}",
        again.text
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn preludelb_bad3_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip preludelb_bad3_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("bad3sc");
    let run = compile_scalac(&sc, &fixture("preludelb_bad3"), &dir, &jar);
    assert_eq!(
        run.text.matches("error:").count(),
        2,
        "scalac should refuse exactly `val a` and `val c`:\n{}",
        run.text
    );
    rejected(
        run,
        "preludelb_bad3 under scalac",
        &[
            "found   : (Cat, Cat) => Cat",
            "required: (Animal, Animal) => Animal",
        ],
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The private runtime.

/// `contains` is the one widened member the `--no-scala-library` runtime also
/// declares (`prelude_seq::add_list_core_private`). `A1`'s erasure is
/// `Object` exactly as `A`'s was, so the private `List` classfile's
/// `contains:(Object)Z` still matches and the program still runs. `indexOf`,
/// `reduce` and `toArray` are not declared there at all and must keep saying
/// so rather than compiling to a call that does not exist.
#[test]
fn preludelb_private_runtime_is_unchanged() {
    if !java_available() {
        eprintln!("skip preludelb_private_runtime_is_unchanged: java not present");
        return;
    }
    let dir = tmp_dir("priv");
    let src = dir.join("Priv.scala");
    fs::write(
        &src,
        r#"object Main {
  def main(args: Array[String]): Unit = {
    val xs: List[Int] = 1 :: 2 :: 3 :: Nil
    println(xs.contains(2))
    println(xs.contains(9))
  }
}
"#,
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let run = out_of(
        Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .arg("--no-scala-library")
            .output()
            .expect("run scala-rs compile"),
    );
    ok(run, "the private-runtime fixture");
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("run java");
    assert_eq!(String::from_utf8_lossy(&o.stdout), "true\nfalse\n");

    // Not declared under the private runtime, and still a diagnostic rather
    // than a call into a method the emitted classfile does not have.
    let src2 = dir.join("Priv2.scala");
    fs::write(
        &src2,
        r#"object Main {
  def main(args: Array[String]): Unit = {
    val xs: List[Int] = 1 :: 2 :: Nil
    println(xs.indexOf(2))
  }
}
"#,
    )
    .unwrap();
    let out2 = dir.join("out2");
    fs::create_dir_all(&out2).unwrap();
    let run2 = out_of(
        Command::new(bin())
            .arg("compile")
            .arg(&src2)
            .args(["-d", out2.to_str().unwrap()])
            .arg("--no-scala-library")
            .output()
            .expect("run scala-rs compile"),
    );
    rejected(run2, "`indexOf` under the private runtime", &["indexOf"]);
    let _ = fs::remove_dir_all(&dir);
}
