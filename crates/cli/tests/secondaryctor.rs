//! E2E tests for the `agent/secondaryctor` slice: `new C(args)` on a
//! *secondary* constructor.
//!
//! `agent/intrinsicqual` left this reduced at the end of its section in
//! `docs/scala-library.md`:
//!
//! ```text
//! class Sec(val a: Int) {
//!   def this(s: String) = this(s.length)
//!   def viaSecondary: Sec = new Sec("abcd")
//! }
//! ```
//!
//! `VerifyError: Bad type on operand stack ... Type 'java/lang/Integer' is not
//! assignable to 'java/lang/String'`, where real scalac 2.13.16 prints `4`.
//!
//! The descriptor was never wrong. `gen_new` receives the constructor the
//! typer picked and emits `invokespecial Sec.<init>:(Ljava/lang/String;)V`,
//! which is what scalac emits. What was wrong is the *argument*:
//! `erasure::method_param_types` answered the question "what parameter types
//! does this `new` adapt its arguments to?" by taking the class's **first**
//! `<init>` member -- always the primary -- and nothing else. So the `String`
//! literal was erased against `Int`, `adapt_box_unbox` wrapped it in `$unbox`,
//! and the emitted call read
//!
//! ```text
//! ldc "abcd"; checkcast java/lang/Integer; Integer.intValue; Integer.valueOf
//! invokespecial Sec."<init>":(Ljava/lang/String;)V
//! ```
//!
//! -- an `Integer` handed to a slot the descriptor declares `String`. It
//! compiles, `javap` looks right at the call, and only the verifier or a run
//! says otherwise. The fix passes the `Apply`'s own symbol (the picked
//! constructor) into `method_param_types`, which is the same symbol the
//! backend already builds the descriptor from, so the two can no longer
//! disagree.
//!
//! Every test here *runs* the program under `java -Xverify:all`. That is not
//! belt-and-braces: this class of defect is invisible to an error count, to a
//! class-file count, and to reading the disassembly at the call site.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.
//! Fixture prefix: `secondaryctor_`.

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
        "scala-rs-secondaryctor-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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
    cached.is_file().then_some(cached)
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// Compile a source string as a file of its own; panic with the diagnostics.
fn compile(tag: &str, src: &str, extra: &[&str]) -> PathBuf {
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        file.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {tag} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// Compile a fixture file by name and return the output directory.
fn compile_fixture(name: &str, extra: &[&str]) -> PathBuf {
    let out = tmp_dir(name);
    let src = fixtures_dir().join(format!("{name}.scala"));
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
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "java -Xverify:all {main} failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(stderr, "", "nothing belongs on stderr:\n{stderr}");
    stdout
}

/// `javap -c -p` of one class in `out`.
fn javap(out: &Path, class: &str) -> String {
    let output = Command::new("javap")
        .args(["-c", "-p", "-cp", &out.display().to_string(), class])
        .output()
        .expect("javap");
    assert!(
        output.status.success(),
        "javap {class} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Both linking modes, as `(tag, compiler flags, runtime classpath extra)`.
///
/// The defect is in erasure, which runs in both modes, so neither half is
/// decoration: an unmodified build of the branch point fails this fixture
/// under `--no-scala-library` exactly as it does against the jar.
fn both_modes() -> Vec<(&'static str, Vec<String>, Option<String>)> {
    let mut modes: Vec<(&'static str, Vec<String>, Option<String>)> =
        vec![("private", vec!["--no-scala-library".into()], None)];
    if let Some(jar) = scala_library_jar() {
        let jar_s = jar.to_str().unwrap().to_string();
        modes.push((
            "jar",
            vec!["--scala-library".into(), jar_s.clone()],
            Some(jar_s),
        ));
    }
    modes
}

// ------------------------------------------------------------- the fixture

/// The fixture, in both modes, against real scalac 2.13.16's own output.
///
/// `tests/fixtures/expected/secondaryctor_new.txt` is literally what
/// `/tmp/scala-2.13.16/bin/scalac` compiling this same file prints. It holds a
/// secondary constructor called from inside the class, from the companion and
/// from an unrelated object; two secondaries whose erased descriptors differ
/// in one parameter; a secondary that delegates to another secondary; a
/// value-class parameter; a default argument; and a plain `new C(primary
/// args)` for every one of those classes, so the primary path is pinned in the
/// same program.
#[test]
fn the_fixture_matches_scalac_in_both_modes() {
    if !java_available() {
        return;
    }
    let name = "secondaryctor_new";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile_fixture(name, &flags);
        let stdout = run_java(&out, cp.as_deref(), "Main");
        assert_eq!(stdout, expected_stdout(name), "[{tag}] stdout for {name}");
        let _ = fs::remove_dir_all(&out);
    }
}

/// The reduction `agent/intrinsicqual` left behind, exactly as written.
///
/// Real scalac 2.13.16 prints `4`; the branch point is a `VerifyError`.
#[test]
fn the_reduction_from_intrinsicqual_prints_four() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  def main(a: Array[String]): Unit = println(new Sec(1).viaSecondary.a)
}
class Sec(val a: Int) {
  def this(s: String) = this(s.length)
  def viaSecondary: Sec = new Sec("abcd")
}
"#;
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("reduction-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), "4\n", "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// The bytecode, not just the answer: the argument reaches the call *as
/// written*, with no unbox/box pair in front of it.
///
/// This is the shape of the defect rather than its symptom. A future change
/// that made the descriptor agree by weakening it -- emitting the primary's
/// `(I)V` and boxing the `String` into it -- would still print nothing and
/// still verify against some other class; this says the call is
/// `ldc "abcd"; invokespecial Sec."<init>":(Ljava/lang/String;)V`, which is
/// what scalac emits.
#[test]
fn no_unbox_is_wrapped_around_a_secondary_ctor_argument() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile(
        "bytecode",
        r#"
class Sec(val a: Int) {
  def this(s: String) = this(s.length)
  def viaSecondary: Sec = new Sec("abcd")
}
"#,
        &["--scala-library", jar_s],
    );
    let text = javap(&out, "Sec");
    let body = text
        .split("public Sec viaSecondary();")
        .nth(1)
        .expect("viaSecondary in javap output");
    assert!(
        body.contains(r#"invokespecial"#) && body.contains("(Ljava/lang/String;)V"),
        "the call must name the secondary's descriptor:\n{body}"
    );
    for forbidden in [
        "checkcast     #",
        "java/lang/Integer.intValue",
        "java/lang/Integer.valueOf",
    ] {
        assert!(
            !body.contains(forbidden),
            "`{forbidden}` has no business in front of a String argument:\n{body}"
        );
    }
    let _ = fs::remove_dir_all(out.parent().unwrap());
}

/// The three call sites the defect distinguishes, each on its own.
///
/// Inside the class, from the companion, and from an unrelated object reach
/// `erase_apply` by different routes; the fixture runs all three together, and
/// this pins each separately so a partial regression names itself.
#[test]
fn a_secondary_ctor_works_from_inside_the_companion_and_outside() {
    if !java_available() {
        return;
    }
    let src = r#"
class Sec(val a: Int) {
  def this(s: String) = this(s.length)
  def inside: Int = new Sec("abcd").a
}
object Sec {
  def companion: Int = new Sec("xyz").a
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Sec(0).inside.toString)
    println(Sec.companion.toString)
    println(new Sec("hello").a.toString)
    println(new Sec(42).a.toString)
  }
}
"#;
    let expect = "4\n3\n5\n42\n";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("sites-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), expect, "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// Two secondaries whose erased descriptors differ in exactly one parameter,
/// plus one that delegates to another secondary rather than to the primary.
///
/// Picking "the first `<init>`" is indistinguishable from picking the right
/// one whenever the alternatives erase alike; these are the shapes where it
/// is not.
#[test]
fn overloaded_and_chained_secondaries_each_get_their_own_parameters() {
    if !java_available() {
        return;
    }
    let src = r#"
class Pair(val k: String, val v: Int) {
  def this(k: String, v: String) = this(k, v.length)
  def this(k: Int, v: Int) = this("i" + k.toString, v)
}
class Chain(val d: Int) {
  def this(s: String) = this(s.length)
  def this(b: Boolean) = this(if (b) "yes" else "no")
}
object Main {
  def main(args: Array[String]): Unit = {
    val a = new Pair("a", 3)
    println(a.k + a.v.toString)
    val b = new Pair("b", "cdef")
    println(b.k + b.v.toString)
    val c = new Pair(5, 9)
    println(c.k + c.v.toString)
    println(new Chain(4).d.toString)
    println(new Chain("abcde").d.toString)
    println(new Chain(true).d.toString)
    println(new Chain(false).d.toString)
  }
}
"#;
    let expect = "a3\nb4\ni59\n4\n5\n3\n2\n";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("overload-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), expect, "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// A value class in a secondary constructor's parameter list.
///
/// `Meters` erases to `int` while the primary takes a `String`, so adapting
/// against the wrong constructor is a `checkcast` in one direction and an
/// unbox in the other. scalac emits `Dist.<init>:(I)V` for the secondary; so
/// do we.
#[test]
fn a_value_class_secondary_parameter_keeps_its_erasure() {
    if !java_available() {
        return;
    }
    let src = r#"
class Meters(val n: Int) extends AnyVal
class Dist(val m: String) {
  def this(x: Meters) = this("m" + (x.n * 2).toString)
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Dist("plain").m)
    println(new Dist(new Meters(6)).m)
  }
}
"#;
    let expect = "plain\nm12\n";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("valueclass-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), expect, "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

// ------------------------------------------------- what must not have moved

/// The primary path, on classes that have no secondary at all.
///
/// `method_param_types` now consults the `Apply`'s symbol first; when that
/// symbol is absent or is not an `<init>` it must fall back to exactly what it
/// did before, or every ordinary `new` in the corpus moves.
#[test]
fn an_ordinary_new_is_unchanged() {
    if !java_available() {
        return;
    }
    let src = r#"
class Plain(val a: Int, val b: String)
class Boxed(val a: Any)
class Generic[A](val a: A)
class Sized(val a: Int, val b: String = "d")
object Main {
  def main(args: Array[String]): Unit = {
    val p = new Plain(1, "x")
    println(p.a.toString + p.b)
    println(new Boxed(7).a.toString)
    println(new Boxed("s").a.toString)
    println(new Generic[Int](3).a.toString)
    println(new Generic[String]("g").a)
    val s = new Sized(2)
    println(s.a.toString + s.b)
  }
}
"#;
    let expect = "1x\n7\ns\n3\ng\n2d\n";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("plain-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), expect, "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// Boxing that the primary path *does* owe is still emitted.
///
/// The fix narrows which constructor supplies the expected types; it must not
/// stop supplying them. A primitive argument reaching an `Any` /
/// type-parameter slot still has to be boxed, and a boxed value reaching a
/// primitive slot still has to be unboxed -- on a secondary constructor as
/// much as on a primary.
#[test]
fn boxing_still_happens_where_the_parameter_asks_for_it() {
    if !java_available() {
        return;
    }
    let src = r#"
class Holder(val v: Any) {
  def this(n: Int, m: Int) = this(n + m)
}
class Unboxer(val n: Int) {
  def this(a: Any) = this(a.asInstanceOf[Int] * 3)
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Holder(5).v.toString)
    println(new Holder(2, 3).v.toString)
    println(new Holder("s").v.toString)
    println(new Unboxer(4).n.toString)
    println(new Unboxer(6: Any).n.toString)
  }
}
"#;
    let expect = "5\n5\ns\n4\n18\n";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("boxing-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), expect, "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// The corpus test this slice turns green, and the *other* direction of the
/// defect: `scala.util.Random`'s primary takes a `java.util.Random` -- a
/// reference -- and the secondary the program calls takes an `Int`.
///
/// `test/files/run/kmpSliceSearch.scala` opens with
/// `new scala.util.Random(java.lang.Integer.parseInt("kmp", 36))`. Adapting
/// against the primary made that a `Box`, so an `Integer` was handed to
/// `scala/util/Random."<init>":(I)V`: `VerifyError: Bad type on operand
/// stack ... Type 'java/lang/Integer' is not assignable to integer`. The whole
/// class-file difference the fix makes there is one instruction -- the
/// `Integer.valueOf` disappears.
///
/// It also shows the defect is not confined to classes declared in source:
/// `Random` is read from the jar. The `StringBuilder` case below does *not*
/// catch it, because every one of that class's constructor parameters is a
/// reference and `box_adaptation` then returns `None` either way.
#[test]
fn a_jar_class_whose_secondary_takes_a_primitive() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile(
        "randomseed",
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a = new scala.util.Random(java.lang.Integer.parseInt("kmp", 36))
    val b = new scala.util.Random(java.lang.Integer.parseInt("kmp", 36))
    println((a.nextInt(1000) == b.nextInt(1000)).toString)
    val c = new scala.util.Random(12345L)
    val d = new scala.util.Random(12345L)
    println((c.nextInt(1000) == d.nextInt(1000)).toString)
    val e = new scala.util.Random(new java.util.Random(7L))
    println((e.nextInt(1000) >= 0).toString)
  }
}
"#,
        &["--scala-library", jar_s],
    );
    let text = javap(&out, "Main$");
    assert!(
        text.contains(r#"scala/util/Random."<init>":(I)V"#),
        "the Int seed must reach the `(I)V` constructor:\n{text}"
    );
    assert!(
        !text.contains("java/lang/Integer.valueOf"),
        "nothing should box the seed:\n{text}"
    );
    assert_eq!(run_java(&out, Some(jar_s), "Main"), "true\ntrue\ntrue\n");
    let _ = fs::remove_dir_all(out.parent().unwrap());
}

/// A secondary constructor on a class read from a *class file* rather than
/// from source, which is how every `new` into the scala-library jar arrives.
///
/// `scala.collection.mutable.StringBuilder` has a primary `(StringBuilder)`
/// and secondaries `()`, `(Int, String)`, `(String)`. All references, so this
/// one already passed on the branch point; it is here as a guard.
#[test]
fn a_secondary_ctor_from_the_jar_is_picked_correctly() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile(
        "jarctor",
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a = new scala.collection.mutable.StringBuilder("seed")
    println(a.toString)
    val b = new scala.collection.mutable.StringBuilder(16, "cap")
    println(b.toString)
    val c = new scala.collection.mutable.StringBuilder()
    c.append("built")
    println(c.toString)
    println(new String("plain"))
    println(new java.lang.StringBuilder("jsb").toString)
  }
}
"#,
        &["--scala-library", jar_s],
    );
    assert_eq!(
        run_java(&out, Some(jar_s), "Main"),
        "seed\ncap\nbuilt\nplain\njsb\n"
    );
    let _ = fs::remove_dir_all(out.parent().unwrap());
}

/// The gap this file used to assert the *rejection* of, now closed by
/// `agent/ctorgaps`: a default argument on a **secondary** constructor.
///
/// `Typer::synthesize_ctor_default_getters` only ever ran over the primary's
/// parameters, so the getter the call site needed was never declared -- and
/// `synthesize_default_getters`, which does run for every `def this(...)`,
/// declared an *instance* `<init>$default$2` on the class being constructed
/// instead. `new Deft("abc")` has no receiver to select that off, so the
/// receiver logic reached for the enclosing object:
/// `value <init>$default$2 is not a member of Main$`.
///
/// The assertion is now on the *value*, which is the only thing that can
/// distinguish a right getter from a wrong one. `docs/scala-library.md`'s
/// wider fixture is `tests/fixtures/ctorgaps_secdefault.scala`; this keeps
/// the original reduction where it was recorded.
#[test]
fn a_default_on_a_secondary_ctor_runs() {
    let src = r#"
class Deft(val p: Int, val q: String) {
  def this(p: String, q: String = "dq") = this(p.length, q)
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Deft("abc").q)
    println(new Deft("abc", "written").q)
    println(new Deft(3, "primary").q)
  }
}
"#;
    if !java_available() {
        return;
    }
    for (tag, flags, cp) in both_modes() {
        let refs: Vec<&str> = flags.iter().map(String::as_str).collect();
        let out = compile(&format!("secdefault-{tag}"), src, &refs);
        assert_eq!(
            run_java(&out, cp.as_deref(), "Main"),
            "dq\nwritten\nprimary\n",
            "mode {tag}"
        );
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// nsc fills a default only when no alternative applies *without* one
/// (`Infer.inferMethodAlternative`), so `new Prefer(1)` below is the
/// **primary** and prints `1`. This compiler weighed both alternatives at once
/// and reported `ambiguous overload for constructor`; `agent/ctorgaps` left
/// that pinned as a rejection and `agent/ctorgaps2` closed it, so the
/// assertion is now on the value.
///
/// `1` rather than `105` is the whole point: the secondary is applicable to
/// one `Int` too, and picking it would be a wrong answer rather than a
/// refusal. `new Prefer(1, 2)` still reaches the secondary, so the rule
/// removes the alternative from the *weighing* and not from the class.
///
/// Both lines are real scalac 2.13.16's output for the same source.
#[test]
fn nsc_prefers_the_alternative_that_needs_no_default() {
    let src = r#"
class Prefer(val n: Int) {
  def this(k: Int, bump: Int = 5) = this(k * 100 + bump)
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Prefer(1).n)
    println(new Prefer(1, 2).n)
  }
}
"#;
    if !java_available() {
        return;
    }
    let out = compile("ctorprefer", src, &["--no-scala-library"]);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success() && text.split_whitespace().collect::<Vec<_>>() == ["1", "102"],
        "expected scalac's `1` then `102`; got status {:?}\n{text}\n{err}",
        output.status
    );
    let _ = fs::remove_dir_all(out.parent().unwrap());
}

/// The other half of that rule: nsc selects the constructor on the **first
/// clause**, so a curried `new` may not be answered by an alternative that
/// clause never fitted.
///
/// `new Three(2)("m")()` folds to `(2, "m")`, which the primary `(Int,
/// String)` accepts exactly -- and with the defaults now filled and the
/// no-default alternative now preferred, that would be a silent wrong answer
/// (`"m"` for `"m/m2"`) where it used to be `ambiguous overload`. Holding the
/// pick to the alternatives whose first clause is one parameter long -- the
/// same set `flatten_curried_new` measured its arity against -- leaves only
/// the secondary. Both lines are real scalac 2.13.16's.
#[test]
fn a_curried_new_is_picked_on_its_first_clause() {
    let src = r#"
class Three(val a: Int, val bc: String) {
  def this(a: Int)(m: String)(tail: String = m + a) = this(a, m + "/" + tail)
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Three(2)("m")().bc)
    println(new Three(2, "z").bc)
  }
}
"#;
    if !java_available() {
        return;
    }
    let out = compile("ctorthree", src, &["--no-scala-library"]);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success() && text.split_whitespace().collect::<Vec<_>>() == ["m/m2", "z"],
        "expected scalac's `m/m2` then `z`; got status {:?}\n{text}\n{err}",
        output.status
    );
    let _ = fs::remove_dir_all(out.parent().unwrap());
}

/// A constructor default in a **later parameter clause**, which
/// `agent/ctorgaps` left open and `agent/ctorgaps2` closed. This test used to
/// assert the defect; it now asserts the value.
///
/// `new Curr(7)()` on the class below emitted an `invokespecial` with one
/// argument for a three-parameter descriptor: `VerifyError: Bad type on
/// operand stack`. The cause was neither the secondary constructor nor the
/// getter -- `fill_defaults_and_implicits` re-read the callee's *unflattened*
/// `paramss` while a `new`'s arguments reach it already flattened, so the
/// second clause was never seen as short. It reproduced on the **primary**
/// constructor of a plain class, with or without a companion, while the
/// identical `def m(a: Int)(b: String = "b" + a)` was filled correctly.
///
/// `"b714"` is `b = "b" + 7` and `c = 7 * 2`, concatenated by the delegation
/// -- so this says the defaults were evaluated with *this* call's `a` and not
/// merely that three arguments reached the descriptor. It is real scalac
/// 2.13.16's answer for the same source.
///
/// `Curr` writes no companion on purpose: a later-clause default may name an
/// earlier parameter, so its `$lessinit$greater$default$n` takes that
/// parameter and cannot be spliced at the call site. nsc synthesizes a `Curr$`
/// to hold it, and `needs_ctor_default_companion` is what does that here.
#[test]
fn a_default_in_a_later_ctor_clause_is_filled() {
    let src = r#"
class Curr(val a: Int, val b: String) {
  def this(a: Int)(b: String = "b" + a, c: Int = a * 2) = this(a, b + c)
}
object Main {
  def main(args: Array[String]): Unit = println(new Curr(7)().b)
}
"#;
    if !java_available() {
        return;
    }
    let out = compile("ctorcurried", src, &["--no-scala-library"]);
    assert!(
        out.join("Curr$.class").exists(),
        "the getter's companion must be emitted; got {:?}",
        fs::read_dir(&out).map(|d| d
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect::<Vec<_>>())
    );
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success() && text.trim() == "b714",
        "expected scalac's `b714`; got status {:?}\n{text}\n{err}",
        output.status
    );
    let _ = fs::remove_dir_all(out.parent().unwrap());
}
