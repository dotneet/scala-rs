//! `Ordering` の companion と summon（`Ordering[T]`）。
//!
//! 報告された 3 形はすべて real scalac 2.13.16 が通す:
//!
//! ```scala
//! Ordering.Int.reverse.compare(1, 2)   // error: value Int is not a member of Ordering[Option[AnyRef]]
//! Ordering[String].compare("a", "b")   // 型検査は通り、実行時 ClassCastException
//! Ordering[Int].reverse.compare(1, 2)  // 同上
//! ```
//!
//! 原因は 1 つで、`agent/integral` の回帰**ではない**（`59d967a` の
//! バイナリでも `value Int is not a member of Ordering` と
//! `ClassCastException` が出る）。
//!
//! `prelude::add_scala_aliases` が入れていたのは nsc の `package object scala`
//! でいう `type Ordering[T] = scala.math.Ordering[T]` だけで、
//! `val Ordering = scala.math.Ordering` が無かった。**項**位置の `Ordering`
//! が trait そのものに解決され、
//!
//! - `Ordering.Int` は trait のメンバを探して失敗する（`scala.math.Ordering.Int`
//!   と完全修飾すれば通っていた）。`agent/integral` が足した
//!   `implicit def Option[T](implicit ord: Ordering[T])` を暗黙変換の探索が
//!   view として拾ったため、エラー文の受け手だけが
//!   `Ordering[Option[AnyRef]]` に化けていた。
//! - `Ordering[String]` は「trait を項に置いた型適用」として**黙って通り**、
//!   codegen が `Ordering$.MODULE$` を `Ordering` に checkcast していた。
//!
//! 直したのは 3 か所:
//! 1. `prelude_ordsummon`: コンパニオン module を項の名前空間にも入れる
//!    （`Integral` / `Fractional` は module 自体が無かったので作る）。
//! 2. `check.rs` の `Module[T]` → `Module.apply[T]` リダイレクト:
//!    pickle から `apply` を供給してから探す。パッケージオブジェクトの
//!    アクセサ（`def Equiv(): Equiv$`）越しでも効くようにした。
//! 3. `implicits.rs`: 第 1 引数リストが implicit の method は view ではない
//!    （SLS 7.3）。`val o: Ordering[Option[Int]] = Ordering.Int` を黙って
//!    通していた。
//!
//! すべて jar に対して `-Xverify:all` で実行し、real scalac の stdout と
//! 突き合わせている。

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
        "scala-rs-ordsummon-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn compile(out: &Path, name: &str, extra: &[&str]) -> (bool, String) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let output = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .args(["-d", out.to_str().unwrap()])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

/// `-Xverify:all`: `Ordering[String]` が返すのは本当に `Ordering` の
/// インスタンスであって `Ordering$` ではない、をベリファイアにも通す。
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
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn jar_run(name: &str) {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip {name}: jar or java not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    assert_eq!(
        run_main(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for {name} (scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// 期待値は real scalac 2.13.16 が印字するものでなければならない。
fn matches_real_scalac(name: &str) {
    let (Some(scalac), Some(jar), true) = (find_scalac(), scala_library_jar(), java_available())
    else {
        eprintln!("skip real-scalac diff {name}: scalac, jar or java not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-nsc"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    assert_eq!(
        run_main(&ref_out, Some(&jar)),
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// jar に対してスニペットを 1 つコンパイルし、診断を返す。
fn compile_src(src: &str, tag: &str) -> (bool, String) {
    let Some(jar) = scala_library_jar() else {
        return (true, String::new());
    };
    let out = tmp_dir(tag);
    let path = out.join("Snippet.scala");
    fs::write(&path, src).unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(&path)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let ok = output.status.success();
    let _ = fs::remove_dir_all(&out);
    (ok, msgs)
}

// ------------------------------------------------------------------ fixtures

#[test]
fn os2_summon_scala_library() {
    jar_run("os2_summon");
}

#[test]
fn os2_summon_matches_real_scalac() {
    matches_real_scalac("os2_summon");
}

/// コンパニオンを項に出せるようにしたことで「なんでも通る」ようにしては
/// いけない。real scalac もこの 5 行を、同じ 5 行で拒否する。
#[test]
fn os2_summon_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip os2_summon_bad: jar not present");
        return;
    };
    let out = tmp_dir("os2_summon_bad");
    let (ok, msgs) = compile(
        &out,
        "os2_summon_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected os2_summon_bad to be rejected, got:\n{msgs}");
    for needle in [
        "type mismatch; found: Ordering$  required: Ordering[Int]",
        "type mismatch; found: Ordering[Int]  required: Ordering[Option[Int]]",
        "value Foo is not a member of Ordering$",
        "value Int is not a member of Numeric$",
        "could not find implicit value of type Ordering[AnyRef]",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for os2_summon_bad, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// 私有ランタイム（`--no-scala-library`）には `scala/math/Ordering` の
/// classfile も `Ordering$` も無い。`prelude_ordsummon` は `library_abi`
/// でゲートしてあり、黙って通すのではなく診断が出る。
#[test]
fn summon_is_diagnosed_without_the_jar() {
    let out = tmp_dir("os2-private");
    let (ok, msgs) = compile(&out, "os2_summon", &["--no-scala-library"]);
    assert!(
        !ok,
        "expected --no-scala-library to reject os2_summon, got:\n{msgs}"
    );
    assert!(
        msgs.contains("not found: value Ordering"),
        "expected `Ordering` to stay unknown without the jar, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------------ snippets

/// 報告された 3 形、そのまま。`ClassCastException` は型検査を通ったあとに
/// 出ていたので、コンパイルできることだけでは足りず、実行して確かめる。
#[test]
fn the_three_reported_forms_run() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        return;
    };
    let out = tmp_dir("os2-repro");
    let path = out.join("Main.scala");
    fs::write(
        &path,
        "object Main {\n  def main(a: Array[String]): Unit = {\n\
         \x20   println(Ordering.Int.reverse.compare(1,2))\n\
         \x20   println(Ordering[String].compare(\"a\",\"b\"))\n\
         \x20   println(Ordering[Int].reverse.compare(1,2))\n\
         \x20 }\n}\n",
    )
    .unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(&path)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(run_main(&out, Some(&jar)), "1\n-1\n1\n");
    let _ = fs::remove_dir_all(&out);
}

/// `Ordering.Option` は導出規則であって view ではない。`sorted` は今までどおり
/// 導出できなければならない（`agent/integral` の
/// `ordering_of_option_is_derived` と同じもの）。
#[test]
fn option_ordering_is_still_derived_but_is_not_a_view() {
    let (ok, msgs) = compile_src(
        "object Snippet { def main(a: Array[String]): Unit = {\n\
         \x20 println(List(Some(2), None, Some(1)).sorted)\n\
         \x20 println(implicitly[Ordering[Option[Int]]].compare(Some(1), None))\n\
         } }\n",
        "os2-optord",
    );
    if msgs.is_empty() {
        return; // no jar on this machine
    }
    assert!(
        ok,
        "expected Ordering[Option[Int]] to resolve, got:\n{msgs}"
    );
    let (bad_ok, bad_msgs) = compile_src(
        "object Snippet { val o: Ordering[Option[Int]] = Ordering.Int }\n",
        "os2-optview",
    );
    assert!(
        !bad_ok,
        "an implicit *clause* must not act as a view, got:\n{bad_msgs}"
    );
    assert!(
        bad_msgs.contains("type mismatch"),
        "expected a type mismatch, got:\n{bad_msgs}"
    );
}

/// `Integral[Int]` は `agent/integral` 以降、trait そのものが項に立って
/// **黙って通り**、実行時 `ClassCastException: scala.math.Integral$ cannot be
/// cast to scala.math.Integral` になっていた（`59d967a` では型エラー）。
#[test]
fn integral_and_fractional_summon() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        return;
    };
    let out = tmp_dir("os2-integral");
    let path = out.join("Main.scala");
    fs::write(
        &path,
        "object Main {\n  def main(a: Array[String]): Unit = {\n\
         \x20   val i: Integral[Int] = Integral[Int]\n\
         \x20   val f: Fractional[Double] = Fractional[Double]\n\
         \x20   println(i.quot(7, 2))\n\
         \x20   println(f.div(1.0, 4.0))\n\
         \x20 }\n}\n",
    )
    .unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(&path)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(run_main(&out, Some(&jar)), "3\n0.25\n");
    let _ = fs::remove_dir_all(&out);
}

/// 既存の `Module[T]` リダイレクト（`List[Int]()` など）は壊れていない。
#[test]
fn module_apply_redirect_still_works() {
    let (ok, msgs) = compile_src(
        "object Snippet { def main(a: Array[String]): Unit = {\n\
         \x20 println(List[Int](1, 2))\n\
         \x20 println(Vector[String](\"a\"))\n\
         \x20 println(Option[Int](3))\n\
         \x20 println(Map[String, Int](\"a\" -> 1))\n\
         } }\n",
        "os2-modapply",
    );
    if msgs.is_empty() {
        return; // no jar on this machine
    }
    assert!(ok, "expected the module factories to compile, got:\n{msgs}");
}
