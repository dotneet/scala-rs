//! Sequence patterns (`case Seq(a, b)`, `case Array(a, rest @ _*)`, ...),
//! `StringOps.map`'s two overloads, and the stable-identifier pattern rule.
//!
//! Every fixture is compiled against the real `scala-library` jar and its
//! output is compared with what nsc 2.13.16 prints for the same source.

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
    let p = std::env::temp_dir().join(format!(
        "scala-rs-seqpat-{tag}-{}-{nanos}",
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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// `-Xverify:all` so a bad `StackMapTable` is a failure, not a silent pass.
fn run_main(out: &Path, jar: Option<&str>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j),
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

fn compile(out: &Path, srcs: &[PathBuf], extra: &[&str]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    let output = cmd.args(extra).output().expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

/// Compile against the jar and check the program's stdout.
fn dual_run(name: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap().to_string();
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, &[src], &["--scala-library", &jar_s]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&out, Some(&jar_s)),
            expected_stdout(name),
            "stdout mismatch for {name}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac: the recorded expectation, nsc's
/// stdout and ours all have to agree.
fn real_scalac_dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff {name}: scalac or jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap().to_string();
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let reference = run_main(&ref_out, Some(&jar_s));
    assert_eq!(
        reference,
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );

    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, &[src], &["--scala-library", &jar_s]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    assert_eq!(
        run_main(&out, Some(&jar_s)),
        reference,
        "stdout differs from real scalac for {name}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, &[src], extra);
    assert!(!ok, "expected compile of {name} to fail, got:\n{msgs}");
    for needle in needles {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

fn accepts(tag: &str, source: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap().to_string();
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (_, msgs) = compile(&out, &[src], &["--scala-library", &jar_s]);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ fixtures

#[test]
fn seqpat_fixture_dual_run() {
    dual_run("seqpat");
}

#[test]
fn seqpat_matches_real_scalac() {
    real_scalac_dual_run("seqpat");
}

#[test]
fn seqpat_map_fixture_dual_run() {
    dual_run("seqpat_map");
}

#[test]
fn seqpat_map_matches_real_scalac() {
    real_scalac_dual_run("seqpat_map");
}

#[test]
fn seqpat_ids_fixture_dual_run() {
    dual_run("seqpat_ids");
}

#[test]
fn seqpat_ids_matches_real_scalac() {
    real_scalac_dual_run("seqpat_ids");
}

/// Stable identifiers and modifiers are ours, not the library's: the private
/// runtime has to produce exactly the same program.
#[test]
fn seqpat_ids_private_runtime() {
    let src = fixtures_dir().join("seqpat_ids.scala");
    let out = tmp_dir("seqpat_ids_nolib");
    let (ok, msgs) = compile(&out, &[src], &["--no-scala-library"]);
    assert!(ok, "compile seqpat_ids without the jar failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&out, None),
            expected_stdout("seqpat_ids"),
            "stdout mismatch for private-runtime seqpat_ids"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The relaxation must not swallow what scalac still rejects: a final class,
/// `String`, or a primitive on either side of a stable-identifier pattern.
#[test]
fn seqpat_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip seqpat_bad: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap().to_string();
    compile_fails(
        "seqpat_bad",
        &["--scala-library", &jar_s],
        &[
            "type mismatch; found: FinalOther  required: ST[Int]",
            "type mismatch; found: String  required: Other",
            "type mismatch; found: FinalOther  required: Tr",
            "type mismatch; found: Other  required: Int",
            "type mismatch; found: Int  required: ST[Int]",
        ],
    );
}

#[test]
fn seqpat_star_must_be_last() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip seqpat_star_bad: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap().to_string();
    compile_fails(
        "seqpat_star_bad",
        &["--scala-library", &jar_s],
        &["`_*` must be the last pattern argument"],
    );
}

/// No stub: without the jar there is no `UnapplySeqWrapper` to call, and the
/// diagnostic has to say so rather than emit code that cannot link.
#[test]
fn seqpat_without_library_is_diagnosed() {
    compile_fails(
        "seqpat_nolib_bad",
        &["--no-scala-library"],
        &["sequence pattern on `Array` needs the real scala-library"],
    );
}

// -------------------------------------------------------------- unit-ish cases

/// `Seq.unapplySeq` reads its elements by index, so a `Vector` reached through
/// `Seq` works. The old `List`-only walk `checkcast`ed it to a `List`.
#[test]
fn a_seq_pattern_binds_the_scrutinees_element_type() {
    accepts(
        "seqpat_elem",
        "object Main {\n\
         \x20 def f(v: Seq[(String, Int)]): Option[String] = v match {\n\
         \x20   case Seq((s, _)) => Some(s)\n\
         \x20   case _ => None\n\
         \x20 }\n\
         \x20 def main(args: Array[String]): Unit = println(f(Vector((\"a\", 1))))\n\
         }\n",
    );
}

/// `rest @ _*` takes the container the extractor's result names: `List` for
/// `List.unapplySeq`, `Seq` for the `Seq` / `Array` factories.
#[test]
fn a_star_pattern_takes_the_extractors_own_container() {
    accepts(
        "seqpat_star_ty",
        "object Main {\n\
         \x20 def a(xs: List[Int]): List[Int] = xs match {\n\
         \x20   case List(_, rest @ _*) => rest.toList\n\
         \x20   case _ => Nil\n\
         \x20 }\n\
         \x20 def b(xs: Seq[Int]): Seq[Int] = xs match {\n\
         \x20   case Seq(_, rest @ _*) => rest\n\
         \x20   case _ => Nil\n\
         \x20 }\n\
         \x20 def main(args: Array[String]): Unit = println(a(List(1, 2)).size + b(Vector(1, 2)).size)\n\
         }\n",
    );
}

/// A user extractor returning `Option[List[T]]` keeps the old cons-list walk.
#[test]
fn a_user_unapply_seq_is_untouched() {
    accepts(
        "seqpat_user",
        "object PairSeq {\n\
         \x20 def unapplySeq(n: Int): Option[List[Int]] = Some(n :: (n + 1) :: Nil)\n\
         }\n\
         object Main {\n\
         \x20 def main(args: Array[String]): Unit = println(10 match {\n\
         \x20   case PairSeq(a, b) => a + b\n\
         \x20   case _ => -1\n\
         \x20 })\n\
         }\n",
    );
}

/// nsc picks the `Char => Char` alternative only when the literal really
/// returns a `Char`; the polymorphic one takes over otherwise.
#[test]
fn string_ops_map_picks_the_alternative_by_the_literals_result() {
    accepts(
        "seqpat_strmap",
        "object Main {\n\
         \x20 val a: String = \"abc\".map(c => c.toUpper)\n\
         \x20 val b: IndexedSeq[String] = \"abc\".map(c => c.toString)\n\
         \x20 def main(args: Array[String]): Unit = println(a + b.size)\n\
         }\n",
    );
}

/// nsc accepts a stable identifier whose type is merely *compatible* with the
/// scrutinee -- two open classes could still have a common subclass.
#[test]
fn a_stable_id_pattern_only_has_to_be_inhabitable() {
    accepts(
        "seqpat_stable",
        "class Other\n\
         class ST[T]\n\
         object Ids { val other = new Other }\n\
         object Main {\n\
         \x20 def f(x: ST[Int]): Int = x match {\n\
         \x20   case Ids.other => 1\n\
         \x20   case _ => 0\n\
         \x20 }\n\
         \x20 def main(args: Array[String]): Unit = println(f(new ST[Int]))\n\
         }\n",
    );
}

/// A class's optional constructor modifier used to swallow the *next*
/// definition's modifiers, so every `final` / `abstract` / `sealed` after the
/// first class in a file was silently dropped.
#[test]
fn modifiers_after_a_class_are_not_swallowed() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip seqpat_mods: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap().to_string();
    let dir = tmp_dir("seqpat_mods");
    let src = dir.join("seqpat_mods.scala");
    // `final` on the *second* class: a stable-identifier pattern of that type
    // is an error only because the class really is final.
    fs::write(
        &src,
        "class First\n\
         final class Second\n\
         object Ids { val s = new Second }\n\
         object Main {\n\
         \x20 def f(x: First): Int = x match {\n\
         \x20   case Ids.s => 1\n\
         \x20   case _ => 0\n\
         \x20 }\n\
         \x20 def main(args: Array[String]): Unit = println(f(new First))\n\
         }\n",
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (_, msgs) = compile(&out, &[src], &["--scala-library", &jar_s]);
    assert!(
        msgs.contains("type mismatch; found: Second  required: First"),
        "`final` on a class that follows another class must survive, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// A constructor access modifier still parses.
#[test]
fn a_constructor_access_modifier_still_parses() {
    accepts(
        "seqpat_ctor_mod",
        "class C private (val x: Int) {\n\
         \x20 def this() = this(1)\n\
         }\n\
         object Main {\n\
         \x20 def main(args: Array[String]): Unit = println(new C().x)\n\
         }\n",
    );
}

#[test]
fn repeated_case_patterns_match_scalac() {
    assert!(
        java_available() && find_scalac().is_some() && scala_library_jar().is_some(),
        "requires Java and Scala 2.13.16"
    );
    real_scalac_dual_run("seqpat_case_repeated");
}

#[test]
fn repeated_case_patterns_reject_missing_fixed_and_wrong_element_types() {
    let scalac = find_scalac().expect("requires Scala 2.13.16");
    let jar = scala_library_jar().expect("requires scala-library");
    for (tag, pattern) in [
        ("missing", "Tagged()"),
        ("wrong", "Tagged(\"x\", s: String)"),
        ("star", "Tagged(rest @ _*, 1)"),
    ] {
        let root = tmp_dir(tag);
        let src = root.join("Bad.scala");
        fs::write(&src, format!("case class Tagged[A](tag: String, xs: A*)\nobject Bad {{ def f(x: Tagged[Int]): Unit = x match {{ case {pattern} => () }} }}\n")).unwrap();
        let rs = root.join("rs");
        let ns = root.join("nsc");
        fs::create_dir_all(&rs).unwrap();
        fs::create_dir_all(&ns).unwrap();
        let (ok, msgs) = compile(
            &rs,
            &[src.clone()],
            &["--scala-library", jar.to_str().unwrap()],
        );
        assert!(!ok, "accepted {tag}: {msgs}");
        let output = Command::new(&scalac)
            .arg(&src)
            .arg("-d")
            .arg(&ns)
            .output()
            .unwrap();
        assert!(!output.status.success(), "nsc accepted {tag}");
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn typed_patterns_agree_with_scalac_on_bounds_and_invariance() {
    let scalac = find_scalac().expect("requires Scala 2.13.16");
    let jar = scala_library_jar().expect("requires scala-library");
    let cases = [
        (
            "int_string",
            false,
            r#"object X { def f(x: Int) = x match { case _: String => 1 } }"#,
        ),
        (
            "int_long",
            false,
            r#"object X { def f(x: Int) = x match { case _: Long => 1 } }"#,
        ),
        (
            "any_string",
            true,
            r#"object X { def f(x: Any) = x match { case _: String => 1 } }"#,
        ),
        (
            "param_string",
            true,
            r#"object X { def f[A](x: A) = x match { case _: String => 1 } }"#,
        ),
        (
            "bounded_string",
            false,
            r#"object X { def f[A <: java.lang.Number](x: A) = x match { case _: String => 1 } }"#,
        ),
        (
            "invariant",
            false,
            r#"final class Box[A]; object X { def f(x: Box[Int]) = x match { case _: Box[String] => 1 } }"#,
        ),
        (
            "open_invariant",
            false,
            r#"class Box[A]; object X { def f(x: Box[Int]) = x match { case _: Box[String] => 1 } }"#,
        ),
        (
            "array",
            false,
            r#"object X { def f(x: Array[Int]) = x match { case _: Array[String] => 1 } }"#,
        ),
        (
            "open_traits",
            true,
            r#"trait A; trait B; object X { def f(x: A) = x match { case _: B => 1 } }"#,
        ),
        (
            "bound_compatible",
            true,
            r#"object X { def f[A <: CharSequence](x: A) = x match { case _: String => 1 } }"#,
        ),
        (
            "finalcov",
            false,
            r#"final class B[+A]; object X {def f(x:B[Int])=x match {case _: B[String] => 1}}"#,
        ),
        (
            "opencov",
            true,
            r#"class B[+A]; object X {def f(x:B[Int])=x match {case _: B[String] => 1}}"#,
        ),
        (
            "wildcard",
            true,
            r#"final class B[A]; object X {def f(x:B[_])=x match {case _: B[String] => 1}}"#,
        ),
        (
            "arrayany",
            false,
            r#"object X {def f(x:Array[Any])=x match {case _: Array[String] => 1}}"#,
        ),
        (
            "binary_indexed_seq",
            true,
            r#"object X { def f[U](x: Iterable[U]) = x match { case _: IndexedSeq[_] => 1 } }"#,
        ),
        (
            "binary_map",
            true,
            r#"object X { def f(x: Map[_ <: AnyRef, (Int => Int, String)]) = x match { case _: Map[String, (Int => Int, String)] @unchecked => 1 } }"#,
        ),
        (
            "binary_set",
            true,
            r#"object X { def f(x: Set[_ <: AnyRef]) = x match { case _: Set[String] @unchecked => 1 } }"#,
        ),
    ];
    for (name, accepted, source) in cases {
        let root = tmp_dir(name);
        let src = root.join("X.scala");
        fs::write(&src, source).unwrap();
        let rs = root.join("rs");
        let ns = root.join("nsc");
        fs::create_dir_all(&rs).unwrap();
        fs::create_dir_all(&ns).unwrap();
        let reference = Command::new(&scalac)
            .arg(&src)
            .arg("-d")
            .arg(&ns)
            .output()
            .unwrap();
        assert_eq!(
            reference.status.success(),
            accepted,
            "nsc {name}: {}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let (ok, msgs) = compile(&rs, &[src], &["--scala-library", jar.to_str().unwrap()]);
        assert_eq!(ok, accepted, "scala-rs {name}: {msgs}");
        fs::remove_dir_all(root).unwrap();
    }
}
