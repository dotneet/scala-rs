//! `agent/varassign`: `name = value` inside `this(...)`, and the assignments
//! that are not assignments.
//!
//! The brief was eight `reassignment to val initBlank` errors in
//! `scala/collection/mutable/AnyRefMap.scala`. `initBlank` is not a `var` we
//! gave the wrong mutability to and not a name we resolved to the wrong
//! symbol: it is a **constructor parameter**, and every one of the eight
//! positions is a *named argument* in a self-constructor delegation --
//! `def this() = this(AnyRefMap.exceptionDefault, 16, initBlank = true)`.
//! `type_ctor_delegation` typed its arguments positionally with no named
//! handling at all, so each pair became an `Assign` against the primary
//! constructor's own parameter (which is in scope in an auxiliary
//! constructor's body, hence the name in the message) and the `Unit`s left
//! behind produced a further `no matching overload for constructor`. The
//! `new C(b = 2, a = 1)` and `extends B(b = 2, a = 1)` paths had both had
//! this fixed years of slices ago; `this(...)` was the third and was missed.
//!
//! The brief also asked for the **opposite** direction, and that is where the
//! value was. A 44-case probe -- every one compiled by this compiler and by
//! real scalac 2.13.16 and compared on accept/reject -- found six
//! disagreements this compiler *accepted*:
//!
//! | program | scalac | this compiler emitted |
//! |---|---|---|
//! | `def v: Int = 1; v = 2` | `value v_= is not a member of C` | `putfield C.v:I`, a field `C` has not got |
//! | `d.v = 2` where `v` is a `def` | `value v_= is not a member of D` | the same, through a `Select` |
//! | `Nil.length = 2` | `value length_= is not a member of object Nil` | the same |
//! | `object O; O = null` | `reassignment to val` | `putfield scala/runtime.O:LO$;` |
//! | a `val` inherited from a class file | `reassignment to val` | `putfield` to someone else's private field |
//! | a `var` inherited from a class file | *accepted* | `putfield B.bv` -- `IllegalAccessError` at run time |
//!
//! One root under all six: `check_reassignment` returned early for any left
//! side that was not a `Term`, on the reasoning that a `Method` left side is
//! an already-resolved `x_=` setter. It is that only when its name ends in
//! `_=`. The last row is the dangerous one, because it is the half that
//! produced no error message: a `var` inherited from a class file arrives as
//! the accessor pair `bv()` / `bv_$eq(int)` around a **private** field, and
//! only the qualified form `this.bv = 5` was ever rewritten to the setter,
//! because only that one is a `Select`. `bv = 5` compiled to a store to a
//! field the subclass may not touch and threw `IllegalAccessError` against a
//! scalac-built superclass -- which is what `varassign_inherited_var_runs`
//! below builds and runs.
//!
//! One more, found on the way and fixed with them: `alt_for_named_args`
//! picked the first alternative that merely *declared* the names, so
//! `class C(a: Int, b: Boolean) { def this(a: Int) = … }` called as
//! `new C(a = 3)` -- or `f(a = 3)` on an overloaded method, or
//! `this(a = 3)` -- reported `missing argument for parameter b` for a call
//! that names the one-parameter alternative exactly. An alternative also has
//! to be *applicable*: what the call leaves uncovered must carry a default,
//! be implicit, or be the repeated tail.
//!
//! Left standing, and recorded in `docs/not-implemented.md`: `val v` beside a
//! hand-written `def v_=`, which scalac accepts as a setter call and this
//! compiler still reports as `reassignment to val`; and assignment to a
//! wildcard-imported `var`, whose receiver is `this` (a `ClassCastException`
//! at run time). Neither is in this family's root.
//!
//! Fixture prefix: `varassign_`, plus `varassign.scala`.

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
        "scala-rs-varassign-{tag}-{}-{nanos}-{seq}",
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

fn compile_rs_cp(src: &Path, out: &Path, jar: &Path, cp: Option<&Path>) -> Run {
    let mut cmd = Command::new(bin());
    cmd.arg("compile")
        .arg(src)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()]);
    if let Some(c) = cp {
        cmd.args(["-cp", c.to_str().unwrap()]);
    }
    out_of(cmd.output().expect("run scala-rs compile"))
}

fn compile_rs(src: &Path, out: &Path, jar: &Path) -> Run {
    compile_rs_cp(src, out, jar, None)
}

fn compile_scalac_cp(sc: &Path, src: &Path, out: &Path, jar: &Path, cp: Option<&Path>) -> Run {
    let classpath = match cp {
        Some(c) => format!("{}:{}", jar.display(), c.display()),
        None => jar.display().to_string(),
    };
    let mut cmd = Command::new(sc);
    cmd.args(["-classpath", &classpath, "-d", out.to_str().unwrap()])
        .arg(src);
    out_of(cmd.output().expect("run scalac"))
}

fn compile_scalac(sc: &Path, src: &Path, out: &Path, jar: &Path) -> Run {
    compile_scalac_cp(sc, src, out, jar, None)
}

/// `-Xverify:all`, so a store to the wrong slot or a link error is a failure
/// here rather than a silent pass.
fn run_java(out: &Path, jar: &Path, extra: Option<&Path>) -> String {
    let cp = match extra {
        Some(e) => format!("{}:{}:{}", out.display(), e.display(), jar.display()),
        None => format!("{}:{}", out.display(), jar.display()),
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

/// Named arguments in `this(...)` -- all named, reordered, mixed with a
/// positional, curried, with an omitted default, with a repeated tail, and
/// naming a one-parameter alternative the primary constructor also covers --
/// plus every accepted shape of `var` assignment. On an unmodified build of
/// the branch point this fixture does not compile: it reports
/// `reassignment to val` at each named argument and then `no matching
/// overload for constructor`.
#[test]
fn varassign_runs() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip varassign_runs: jar or java not present");
        return;
    };
    let dir = tmp_dir("run");
    ok(compile_rs(&fixture("varassign"), &dir, &jar), "varassign");
    assert_eq!(run_java(&dir, &jar, None), expected_stdout("varassign"));
    let _ = fs::remove_dir_all(&dir);
}

/// The expected file is scalac's. `Eff.log` in the fixture is the part this
/// asserts that a weaker test would not: SLS 6.6.1 evaluates arguments left
/// to right *as written*, so `this(b = …, a = …)` must log `b;a;` while `b`
/// still lands in `b`'s slot. Placing the arguments and then evaluating them
/// prints `a;b;` and passes every type check.
#[test]
fn varassign_expected_output_is_scalacs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip varassign_expected_output_is_scalacs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("scalac");
    ok(
        compile_scalac(&sc, &fixture("varassign"), &dir, &jar),
        "varassign under scalac",
    );
    assert_eq!(run_java(&dir, &jar, None), expected_stdout("varassign"));
    let _ = fs::remove_dir_all(&dir);
}

/// A delegation that omits a default has to reach codegen with the flat
/// argument list the `<init>` descriptor promises, and the named arguments
/// have to be *lifted into locals in written order* rather than reordered in
/// place. Both are visible in the disassembly, and both must match scalac's.
#[test]
fn varassign_delegation_emits_the_flat_descriptor() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), javap_available()) else {
        eprintln!(
            "skip varassign_delegation_emits_the_flat_descriptor: jar, scalac or javap absent"
        );
        return;
    };
    let dir = tmp_dir("emit");
    let mine = dir.join("mine");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&mine).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    ok(compile_rs(&fixture("varassign"), &mine, &jar), "varassign");
    ok(
        compile_scalac(&sc, &fixture("varassign"), &theirs, &jar),
        "varassign under scalac",
    );
    for out in [&mine, &theirs] {
        let text = javap(out, "C");
        let who = if out == &mine { "scala-rs" } else { "scalac" };
        // `def this(n: Int) = this(a = "n" + n)` leaves `b` and `c` to the
        // one-argument alternative and to `c`'s default; whichever route the
        // call takes, the constructor it reaches is invoked at the class's
        // one flat descriptor.
        assert!(
            text.contains("(Ljava/lang/String;IZ)V"),
            "{who} should invoke C's flat three-parameter <init>:\n{text}"
        );
        // The written order is kept by lifting into locals, so the two
        // effectful arguments are evaluated before the `invokespecial`, not
        // interleaved with it.
        let ctor = body(&text, "public C();");
        assert!(
            ctor.matches("astore").count() + ctor.matches("istore").count() >= 2,
            "{who} should lift the reordered arguments into locals:\n{ctor}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The half that produced no error message: a `var` inherited from a class
// file, assigned by simple name.

/// The superclass is built by **real scalac**, so its backing field is
/// `private` exactly as a real dependency's would be. The branch point
/// emitted `putfield B.bv` here and threw
/// `IllegalAccessError: class E tried to access private field B.bv`
/// on the first call; nothing in the type checker said a word.
#[test]
fn varassign_inherited_var_runs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip varassign_inherited_var_runs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("inherit");
    let lib_src = dir.join("Lib.scala");
    fs::write(
        &lib_src,
        "class B { var bv: Int = 1; val bc: Int = 2 }\ntrait T { var tv: Int = 3; val tc: Int = 4 }\n",
    )
    .unwrap();
    let use_src = dir.join("Use.scala");
    fs::write(
        &use_src,
        r#"class E extends B with T {
  def g(): Unit = { bv = 5; tv = 6 }
}
object Main {
  def main(args: Array[String]): Unit = {
    val e = new E
    e.g()
    println("" + e.bv + e.tv + e.bc + e.tc)
  }
}
"#,
    )
    .unwrap();
    let lib = dir.join("lib");
    let user = dir.join("use");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&user).unwrap();
    ok(
        compile_scalac(&sc, &lib_src, &lib, &jar),
        "Lib under scalac",
    );
    ok(
        compile_rs_cp(&use_src, &user, &jar, Some(&lib)),
        "Use against a scalac-built Lib",
    );
    // The store must be the setter call, not a field write: `bv`'s field is
    // private to `B`.
    if javap_available() {
        let text = javap(&user, "E");
        assert!(
            text.contains("bv_$eq"),
            "`bv = 5` must invoke B's setter, not store the field:\n{text}"
        );
        assert!(
            !text.contains("putfield      #") || !text.contains("Field B.bv"),
            "`bv = 5` must not putfield B's private field:\n{text}"
        );
    }
    assert_eq!(run_java(&user, &jar, Some(&lib)), "5624\n");
    let _ = fs::remove_dir_all(&dir);
}

/// The same arrangement with `val`s instead: assigning one of those is the
/// error scalac reports, and the branch point compiled it to a `putfield`.
#[test]
fn varassign_inherited_val_is_reported() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip varassign_inherited_val_is_reported: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("inheritbad");
    let lib_src = dir.join("Lib.scala");
    fs::write(
        &lib_src,
        "class B { val bc: Int = 2 }\ntrait T { val tc: Int = 4 }\n",
    )
    .unwrap();
    let lib = dir.join("lib");
    fs::create_dir_all(&lib).unwrap();
    ok(
        compile_scalac(&sc, &lib_src, &lib, &jar),
        "Lib under scalac",
    );
    for (name, src) in [
        ("class", "class E extends B { def g(): Unit = { bc = 5 } }"),
        ("trait", "class F extends T { def g(): Unit = { tc = 5 } }"),
    ] {
        let bad = dir.join(format!("Bad{name}.scala"));
        fs::write(&bad, src).unwrap();
        let out = dir.join(format!("out{name}"));
        fs::create_dir_all(&out).unwrap();
        rejected(
            compile_rs_cp(&bad, &out, &jar, Some(&lib)),
            &format!("assigning an inherited {name} `val` from a class file"),
            &["reassignment to val"],
        );
        rejected(
            compile_scalac_cp(&sc, &bad, &out, &jar, Some(&lib)),
            &format!("assigning an inherited {name} `val`, under scalac"),
            &["reassignment to val"],
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Negatives. Each is paired with the same file under real scalac, so the
// sentence being asserted is nsc's and not ours.

/// The five the branch point **accepted**, in one file. Every one of them
/// reached the backend and emitted a store to a field that does not exist.
#[test]
fn varassign_bad_is_reported() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip varassign_bad_is_reported: jar not present");
        return;
    };
    let dir = tmp_dir("bad");
    let run = compile_rs(&fixture("varassign_bad"), &dir, &jar);
    assert!(
        !run.ok,
        "expected varassign_bad to be rejected, but it compiled:\n{}",
        run.text
    );
    for n in [
        "reassignment to val v",
        "reassignment to val x",
        "reassignment to val O",
        "reassignment to val length",
        "reassignment to val w",
    ] {
        assert!(
            run.text.contains(n),
            "expected varassign_bad to report {n:?}:\n{}",
            run.text
        );
    }
    // Exactly five: one per assignment, and no cascade behind any of them.
    assert!(
        run.text.contains("5 error(s)"),
        "varassign_bad should report exactly five errors:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}

/// scalac rejects the same file at the same five positions. Without this the
/// fixture only records what this compiler happens to do.
#[test]
fn varassign_bad_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip varassign_bad_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("badsc");
    rejected(
        compile_scalac(&sc, &fixture("varassign_bad"), &dir, &jar),
        "varassign_bad under scalac",
        &[
            "value v_= is not a member of C",
            "reassignment to val",
            "value length_= is not a member of object Nil",
            "value w_= is not a member of Derived",
            "5 errors",
        ],
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The named-argument alternative bug this slice fixed on the way, in all
/// three of the paths that share `alt_for_named_args`: a constructor
/// delegation, a `new`, and an overloaded method. Each names the alternative
/// that takes exactly those parameters, while an alternative that merely
/// declares the same name also exists.
#[test]
fn varassign_named_args_pick_an_applicable_alternative() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!(
            "skip varassign_named_args_pick_an_applicable_alternative: jar, scalac or java absent"
        );
        return;
    };
    let dir = tmp_dir("alt");
    let src = dir.join("Alt.scala");
    fs::write(
        &src,
        r#"class C(a: Int, b: Boolean) {
  def this(a: Int) = this(a, b = true)
  def this(s: String) = this(a = 3)
  override def toString = a + "/" + b
}
object Main {
  def f(a: Int, b: Boolean): String = "2:" + a + b
  def f(a: Int): String = "1:" + a
  def main(args: Array[String]): Unit = {
    println(new C("s"))
    println(new C(a = 4))
    println(f(a = 5))
    println(f(a = 6, b = false))
  }
}
"#,
    )
    .unwrap();
    let mine = dir.join("mine");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&mine).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    ok(compile_rs(&src, &mine, &jar), "the alternative fixture");
    ok(
        compile_scalac(&sc, &src, &theirs, &jar),
        "the alternative fixture under scalac",
    );
    let got = run_java(&mine, &jar, None);
    assert_eq!(
        got,
        run_java(&theirs, &jar, None),
        "this compiler and scalac must pick the same alternative"
    );
    assert_eq!(got, "3/true\n4/true\n1:5\n2:6false\n");
    let _ = fs::remove_dir_all(&dir);
}

fn javap(dir: &Path, cls: &str) -> String {
    let o = Command::new("javap")
        .args(["-p", "-c", "-cp", dir.to_str().unwrap(), cls])
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
fn body(javap_out: &str, decl_suffix: &str) -> String {
    let mut lines = javap_out
        .lines()
        .skip_while(|l| !l.trim().ends_with(decl_suffix));
    let decl = lines
        .next()
        .unwrap_or_else(|| panic!("no `{decl_suffix}` in:\n{javap_out}"));
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
