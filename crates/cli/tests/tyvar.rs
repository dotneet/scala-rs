//! 型変数の遅延解決（nsc の undetermined type variables）。
//!
//! 引数はオーバーロード解決を型で駆動するために期待型なしで型付けされる。その
//! 結果、`Map.empty` のような多相参照は自分の型パラメータを抱えたまま
//! （`Map[K, V]`）引数位置に届く。nsc はそれを「未確定の型変数」として持ち回り、
//! 候補を選び終えてからパラメータ型で解く。ここではその経路と、逆向き
//! （呼び出し側の型パラメータがまだ未確定な `PartialFunction[Int, ?B]`）の
//! 経路の両方を固定する。
//!
//! フィクスチャは実 `scala-library` の jar に対してコンパイルし、出力を
//! nsc 2.13.16 が同じソースに対して出すものと比較する。

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
        "scala-rs-tyvar-{tag}-{}-{nanos}",
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

fn run_main(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
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

fn compile(out: &Path, jar: &Path, srcs: &[PathBuf]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    let output = cmd
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

/// Compile against the real jar and check the program's output against the
/// recorded nsc output.
fn dual_run(name: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, &jar, &[src]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    if java_available() {
        let expected =
            fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
                .unwrap();
        assert_eq!(run_main(&out, &jar), expected, "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// Compile the named snippet and require no error.
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
    let (_, msgs) = compile(&out, &jar, &[src]);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Compile the named snippet and require the given diagnostic.
fn rejects(tag: &str, source: &str, needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, &jar, &[src]);
    assert!(!ok, "expected {tag} to be rejected, got:\n{msgs}");
    assert!(
        msgs.contains(needle),
        "expected {needle:?} in diagnostics for {tag}, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn fixture_fails(name: &str, needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, &jar, &[src]);
    assert!(!ok, "expected compile of {name} to fail");
    assert!(
        msgs.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------------ fixtures

#[test]
fn tyvar_fixture_dual_run() {
    dual_run("tyvar");
}

/// The variables must not be "solved" by giving up. Each of these is rejected
/// by nsc too.
#[test]
fn an_unsolvable_type_variable_is_still_an_error() {
    fixture_fails(
        "tyvar_unsolved_bad",
        "no matching overload for (Map[String, Int])Int with arguments (List[A])",
    );
}

// ------------------------------------------------------------ unit-ish cases

/// The core case: an argument typed with no expected type keeps its own type
/// parameters, and the parameter it fills is what fixes them.
#[test]
fn a_polymorphic_reference_in_argument_position_is_solved_by_the_parameter() {
    accepts(
        "tyvar_empty_arg",
        "object M {\n\
         \x20 def take(m: Map[String, Int]): Int = m.size\n\
         \x20 val r = take(Map.empty)\n\
         }\n",
    );
}

/// The same for a polymorphic `apply` with no arguments of its own.
#[test]
fn an_empty_apply_is_solved_by_the_parameter() {
    accepts(
        "tyvar_empty_apply",
        "object M {\n\
         \x20 def take(v: Vector[String]): Int = v.length\n\
         \x20 val r = take(Vector())\n\
         }\n",
    );
}

/// A variable an inner call could not solve is still undetermined for the call
/// that encloses it.
#[test]
fn a_variable_leaks_out_of_a_nested_call() {
    accepts(
        "tyvar_nested",
        "object M {\n\
         \x20 def id[T](x: T): T = x\n\
         \x20 def take(m: Map[String, Int]): Int = m.size\n\
         \x20 val r = take(id(Map.empty))\n\
         }\n",
    );
}

/// A variable that reached the *result* is solved from the expected type.
#[test]
fn the_expected_type_solves_a_variable_that_reached_the_result() {
    accepts(
        "tyvar_from_expected",
        "object M {\n\
         \x20 def f[T](x: T): List[T] = List(x)\n\
         \x20 val r: List[Map[String, Int]] = f(Map.empty)\n\
         }\n",
    );
}

/// Overload selection itself runs through the variables: neither alternative
/// is applicable to a fixed `Seq[A]`, but one of them is to `Seq[?A]`.
#[test]
fn overload_selection_sees_through_an_undetermined_variable() {
    accepts(
        "tyvar_overload",
        "object M {\n\
         \x20 def f(x: Seq[Int]): Int = x.sum\n\
         \x20 def f(x: String): Int = x.length\n\
         \x20 val r = f(Seq.empty)\n\
         }\n",
    );
}

/// Constructor arguments take the same path.
#[test]
fn a_constructor_argument_is_solved_by_its_parameter() {
    accepts(
        "tyvar_ctor",
        "class Box(val m: Map[String, Int], val v: Vector[String])\n\
         object M { val r = new Box(Map.empty, Vector.empty) }\n",
    );
}

/// A type parameter an enclosing definition binds is a *type*, not a variable.
/// Solving it from the parameter would accept a program nsc rejects.
#[test]
fn an_enclosing_methods_type_parameter_is_not_a_variable() {
    rejects(
        "tyvar_enclosing_bad",
        "object M {\n\
         \x20 def take(m: Map[String, Int]): Int = m.size\n\
         \x20 def g[K](m: Map[K, Int]): Int = take(m)\n\
         }\n",
        "no matching overload",
    );
}

/// Nor may a method solve its *own* type parameter from the parameter it hands
/// a recursive call to.
#[test]
fn a_recursive_call_does_not_solve_its_own_type_parameter() {
    rejects(
        "tyvar_recursive_bad",
        "object M {\n\
         \x20 def take(m: Map[String, Int]): Int = m.size\n\
         \x20 def rec[T](x: T, m: Map[T, Int]): Int = take(m)\n\
         }\n",
        "no matching overload",
    );
}

/// A variable whose shape does not match the parameter is not solvable, and
/// stays an error.
#[test]
fn a_variable_the_parameter_cannot_pin_stays_an_error() {
    rejects(
        "tyvar_shape_bad",
        "object M {\n\
         \x20 def take(m: Map[String, Int]): Int = m.size\n\
         \x20 val r = take(List.empty)\n\
         }\n",
        "no matching overload",
    );
}

// ------------------------------------- the callee's own undetermined variables

/// The other half of nsc's undetermined variables: `List.collect`'s `B` is
/// still open when the literal is checked. Erasing it to `Any` (what
/// `relax_open_tparams` used to do) loses the result type; solving it from the
/// argument keeps it.
#[test]
fn a_callees_open_type_parameter_is_solved_from_the_argument() {
    accepts(
        "tyvar_collect",
        "object M {\n\
         \x20 val xs = List(1, 2, 3, 4)\n\
         \x20 val r: List[String] = xs.collect { case n if n % 2 == 0 => n.toString }\n\
         }\n",
    );
}

/// The solution has to be the argument's own result type, not `Any`: a
/// declared `List[Int]` must still reject a `String` result.
#[test]
fn solving_a_callees_type_parameter_does_not_widen_the_result() {
    rejects(
        "tyvar_collect_bad",
        "object M {\n\
         \x20 val xs = List(1, 2, 3, 4)\n\
         \x20 val r: List[Int] = xs.collect { case n => n.toString }\n\
         }\n",
        "type mismatch",
    );
}
