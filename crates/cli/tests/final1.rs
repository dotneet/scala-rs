//! `agent/final1` スライスの回帰テスト。slick の「コレクション引数まわり」
//! 7 件のうち 6 件の根をまとめてある。
//!
//! * 自己別名 `class C { self => … self(i) … }` は `C.this.type`。
//!   `Select` 側だけがこれをクラスへ widen しており、適用側
//!   (`resolve_overload`) は `_ => None` で止まって
//!   `value apply is not a member of C.this.type` を出していた
//!   （slick `util/ConstArray.scala:276`）。
//! * 期待型のない位置でも、implicit 節しか残っていない式の未確定型変数は
//!   nsc の `adaptToImplicitMethod` と同じく先に確定する。下限を持つものは
//!   その下限になる（`Nothing` になるものは開いたまま）ので、
//!   `toArray[R >: T : ClassTag]` は `Array[String]` になり、
//!   `withPreparedInsertStatement` の 2 つの overload を区別できる
//!   （slick `jdbc/JdbcActionComponent.scala:725`）。
//! * `typing_call_args`（「引数を型付け中」）は typer のフラグであって式の
//!   ものではないので、引数の途中で走る遅延シグネチャ補完がこれを引き継いで
//!   いた。結果、前方参照された `def … = ….map(…).flatten` の *推論結果型* が
//!   `((Option[X]) => IterableOnce[B])Seq[B]` という未適用のメソッド型に
//!   なっていた（slick `jdbc/JdbcModelBuilder.scala:159`。93 行目はこの
//!   カスケード）。
//! * 同じ型パラメータに 2 つの引数が寄与するとき、および宣言された下限と
//!   join するとき、引数自身の未確定変数は先に下限へ落とす。
//!   `m.getOrElse(k, Seq.empty)` が `Seq[AnyRef]` になっていた
//!   （slick `compiler/MergeToComprehensions.scala:218`）。
//! * case class でないクラスは、コンパニオンに抽出子があるならコンストラクタ
//!   パターンではなく抽出子で照合する（SLS 8.1.6/8.1.7）。
//!   `ConstArray(disc, map)` が `Array[Any]` と `Int` を束縛していた
//!   （slick `compiler/ExpandSums.scala:245`）。
//! * 受け手（レシーバ）が持ち込んだ未確定変数も、結果の *不変* 位置では
//!   期待型が引数より強い。`Set() ++ opt` が `Set[SqlType]` のままで、
//!   不変な `Set` が期待型を拒否していた
//!   （slick `jdbc/JdbcModelBuilder.scala:279` の半分）。
//! * 変換探索の `open_conversion_fit` は、解くべき変数が両側とも空でも
//!   `Unify` に判定させていた。ワイルドカードは何にでも unify するので
//!   `Option.option2Iterable` が `Option[Default[_]] =>
//!   IterableOnce[ColumnOption[Nothing]]` を名乗り、単相の
//!   `Set#++(IterableOnce[A]): Set[A]` が適用可能になって、
//!   `Set() ++ … ++ dflt` の鎖が `Set[ColumnOption[Nothing]]` に落ちていた
//!   （同 279 のもう半分）。
//!
//! ブリーフ通り fixture は 1 ファイルにまとめてある（実 scalac 1 回 1.8 秒）。
//! `Set` / `Map` / `ClassTag` / `IndexedSeq` を使うので `--scala-library`
//! モードのみ。ヘルパは `crates/cli/tests/ovl4.rs` に倣う。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-final1-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

/// `-Xverify:all`: overload を取り違えて消去記述子がずれた codegen は
/// 出力差ではなく `VerifyError` になる。
fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_diagnostics(name: &str) -> Option<String> {
    let jar = scala_library_jar()?;
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
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
    Some(err)
}

#[test]
fn fixtures_final1_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip final1: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("final1", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, jar_s),
        expected_stdout("final1"),
        "stdout mismatch for library-ABI final1"
    );
    let _ = fs::remove_dir_all(&out);
}

/// 同じ fixture を実 scalac 2.13.16 に通し、記録した期待値・scalac の出力・
/// こちらの出力の 3 つが一致することを確かめる。
#[test]
fn real_scalac_dual_run_final1() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff final1: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("final1.scala");
    let ref_out = tmp_dir("final1-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile final1");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, jar_s);
    assert_eq!(
        reference,
        expected_stdout("final1"),
        "recorded expectation for final1 does not match real scalac"
    );

    let out = compile_fixture_with("final1", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, jar_s),
        reference,
        "stdout differs from real scalac for final1"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// 緩めた側の反対側。実 scalac 2.13.16 もこの 3 件を拒否する
/// (`Main.NoApply does not take parameters` /
/// `found: Some[String] required: IterableOnce[Int]` /
/// `found: Option[Main.DefaultOpt[_]] required: IterableOnce[Main.ColOpt[Nothing]]`)。
#[test]
fn final1_bad_is_still_rejected() {
    let Some(err) = compile_diagnostics("final1_bad") else {
        eprintln!("skip: scala-library jar not available");
        return;
    };
    assert!(
        err.contains("value apply is not a member of NoApply.this.type"),
        "a self alias whose class has no `apply` must still be reported: {err}"
    );
    assert!(
        err.contains("found: Set[String]  required: Set[Int]"),
        "the expected type must not override an argument solution that does not \
         conform to it: {err}"
    );
    assert!(
        err.contains("found: Option[DefaultOpt[_]]  required: IterableOnce[ColOpt[Nothing]]"),
        "`option2Iterable` must not answer a view whose result does not actually \
         conform: {err}"
    );
}
