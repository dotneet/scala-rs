//! A view brought into scope by `import <a value>._`.
//!
//! slick's `MySQLProfile` / `JdbcStatementBuilderComponent` write
//! `import seq.integral._` and then `increment < zero`, `start - increment`,
//! `-increment`. Every one of those reported `value <op> is not a member of T`.
//! Four separate causes, all in the same shape -- a conversion that is an
//! *instance member of a generic class*, reached through a value:
//!
//!  * **A jar class's implicits were never in scope.** Members are read from
//!    the pickle one name at a time, and *nothing ever names an implicit* --
//!    it is found by searching a scope. So `Numeric#mkNumericOps` and
//!    `Ordering#mkOrderingOps` were in no member list at all, and neither was
//!    `Option.option2Iterable`: `where.reduceLeft(f)` and `c.where.toSeq` on
//!    an `Option[Node]` (slick's `JdbcStatementBuilderComponent` again) were
//!    `value reduceLeft is not a member of Option[Node]`. Both entry points --
//!    `import <a value>._` and the companion in a type's implicit scope --
//!    now ask the pickle which of its names are implicit and complete those,
//!    through the ordinary on-demand path and only for a name the class has
//!    no member for, so a hand-written prelude declaration still wins.
//!    Primitive companions are left out: `object Int`'s implicits are the
//!    numeric widenings, which `weak_conforms` already implements, and as
//!    views they only make `n + ":"` ambiguous.
//!  * **The candidate kept its owner's type parameters.** `class Box[T] {
//!    implicit def mkOps(lhs: T): Ops[T] }` reached through `b: Box[Int]` is
//!    an `Int => Ops[Int]`, and the value is the only thing that says so.
//!    Left as `Box`'s own `T` it matched nothing.
//!  * **An overridden conversion counted twice.** `Integral[T]` narrows
//!    `Numeric[T]#mkNumericOps`'s result from `NumericOps` to `IntegralOps`,
//!    and both names are in scope after the import. The two results are
//!    different classes declaring different `unary_-` symbols, so the existing
//!    "one conversion reached by two routes" rule did not apply and the search
//!    gave up. In nsc there is one member: the derived one.
//!  * **A member of a class nested in a generic class was unreadable.**
//!    `Ordering[T]#OrderingOps` declares `def <(rhs: T)` at *`Ordering`'s*
//!    parameter -- `OrderingOps` has none of its own -- so `T` was a name
//!    nothing mapped and every member of it failed to install. It is read at
//!    the outer class's parameters and substituted, like the conversion, at
//!    the prefix the value was reached through.
//!
//! A fifth thing was already wrong before any of this: the conversion was
//! emitted as a *bare name*, so codegen loaded `this` and cast it --
//! `class Main$ cannot be cast to class NoTp` from a program that typechecked.
//! The reference now names the value it was imported from.
//!
//! The fixtures are dual-run: compiled against the real `scala-library` jar
//! and (where the private runtime can back them) on it, under `-Xverify:all`,
//! with their stdout compared against nsc 2.13.16's.

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
        "scala-rs-tail2-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
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
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile(out: &Path, jar: Option<&Path>, srcs: &[PathBuf]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
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

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn accepts(tag: &str, source: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (_, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Runs an emitted `Main` in both modes and compares its stdout.
fn dual_run(name: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let exp = expected(name);

    let priv_out = tmp_dir("priv");
    let (ok, msgs) = compile(&priv_out, None, std::slice::from_ref(&src));
    assert!(ok, "compile {name} (private runtime) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&priv_out, None),
            exp,
            "stdout mismatch for {name} on the private runtime"
        );
    }
    let _ = fs::remove_dir_all(&priv_out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name} (jar): scala-library jar not present");
        return;
    };
    let jar_out = tmp_dir("jar");
    let (ok, msgs) = compile(&jar_out, Some(&jar), &[src]);
    assert!(ok, "compile {name} (jar) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&jar_out, Some(&jar)),
            exp,
            "stdout mismatch for {name} against the jar"
        );
    }
    let _ = fs::remove_dir_all(&jar_out);
}

// ------------------------------------------------------------------ fixtures

/// The owner's type parameters, the receiver, the override and the nested
/// result class are all plain language rules: no library type is involved, so
/// this runs in both modes.
#[test]
fn t2_lang_runs_in_both_modes() {
    dual_run("t2_lang");
}

/// slick's own shape: `import seq.integral._` over `scala.math.Integral`,
/// whose operators live on classes nested inside `Numeric` / `Ordering`.
/// Library-ABI only.
#[test]
fn t2_lib_runs_against_the_jar() {
    let name = "t2_lib";
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    if java_available() {
        assert_eq!(run_main(&out, Some(&jar)), expected(name));
    }
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime has no `scala.math.Integral` at all; it must say so
/// rather than compile something it cannot back.
#[test]
fn t2_lib_without_library_is_error() {
    let src = fixtures_dir().join("t2_lib.scala");
    let out = tmp_dir("t2_lib_nolib");
    let (ok, msgs) = compile(&out, None, &[src]);
    assert!(!ok, "t2_lib should not compile without the jar:\n{msgs}");
    assert!(
        msgs.contains("not found: type Integral"),
        "expected a diagnostic naming Integral, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Reading the view at the value narrows it; it does not widen it. nsc
/// 2.13.16 reports the same three.
#[test]
fn t2_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip t2_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("t2_bad.scala");
    let out = tmp_dir("t2_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "t2_bad should not compile, got:\n{msgs}");
    for needle in [
        "value dbl is not a member of \"s\"",
        "no matching overload for (Int)String",
        "value dbl is not a member of T",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- unit-ish cases

/// The import has to ask the pickle which of a jar class's members are
/// implicit: nothing else ever names them. `Ordering[T]#mkOrderingOps` is the
/// smallest case.
#[test]
fn t2_wildcard_import_brings_a_jar_class_implicits_into_scope() {
    accepts(
        "t2_scope",
        "object Main {\n\
         \x20 def cmp[T](ord: Ordering[T], a: T, b: T): Boolean = {\n\
         \x20   import ord._\n\
         \x20   a < b }\n\
         \x20 def main(a: Array[String]): Unit = println(cmp(Ordering.Int, 1, 2)) }\n",
    );
}

/// `Integral[T]` overrides `Numeric[T]#mkNumericOps` with a narrower result.
/// Both names are in scope after the import; there is one candidate.
#[test]
fn t2_overridden_conversion_is_one_candidate() {
    accepts(
        "t2_override",
        "object Main {\n\
         \x20 def f[T](i: Integral[T], x: T): T = {\n\
         \x20   import i._\n\
         \x20   x * x }\n\
         \x20 def main(a: Array[String]): Unit = println(f(Numeric.IntIsIntegral, 6)) }\n",
    );
}

/// The companion in a type's implicit scope needs the same treatment:
/// `Option.option2Iterable` is what gives an `Option` the `Iterable` members.
#[test]
fn t2_companion_implicits_are_supplied_from_the_pickle() {
    accepts(
        "t2_optview",
        "object Main {\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val o: Option[String] = Some(\"a\")\n\
         \x20   println(o.reduceLeft((x: String, y: String) => x + y))\n\
         \x20   println(o.toSeq)\n\
         \x20   println(Seq(\"z\") ++ o) } }\n",
    );
}

/// `object Int`'s implicits are the numeric widenings; as views they compete
/// with `any2stringadd` for `+`. `n + \":\"` must still resolve.
#[test]
fn t2_primitive_companions_stay_out_of_the_view_search() {
    accepts(
        "t2_intplus",
        "object Main {\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val n = 3\n\
         \x20   println(n + \":\" + 4L + \":\" + 'c') } }\n",
    );
}

/// `Numeric[T]#IntegralOps` is nested in the *generic* trait, so its members
/// are written at `Numeric`'s `T`. `quot` also proves the `Integral`-only half
/// of the override is what is being used.
#[test]
fn t2_nested_class_members_read_at_the_outer_parameters() {
    accepts(
        "t2_nested",
        "object Main {\n\
         \x20 def divmod[T](i: Integral[T], a: T, b: T): (T, T) = {\n\
         \x20   import i._\n\
         \x20   (a / b, a % b) }\n\
         \x20 def main(x: Array[String]): Unit = println(divmod(Numeric.IntIsIntegral, 7, 2))\n\
         }\n",
    );
}
