//! 「implicit が見つからない」「メンバにアクセスできない」で報告された slick の
//! エラーを最小再現した回帰テスト。すべて実 scalac 2.13.16 が受理する形で、
//! `tests/fixtures/implfind.scala` に 8 ケースまとめてある。
//!
//! 根（診断の言葉と根が違うものが多い）:
//!
//! 1. **適用済みの抽象型メンバが自分の上限に適合しない。**
//!    `type CT[T] <: TT[T]` を `CT[U]` に適用したとき、上限は `TT[T]` の
//!    ままで比較されていた（`T` は `CT` 自身のパラメータ）。`CT[U] <: TT[U]`
//!    が偽なので、文脈境界が入れた evidence がその境界を満たさない --
//!    つまり implicit 探索ではなく **部分型判定** が根。slick の
//!    `implicitly[BaseColumnType[U]]` と `TypedType[Boolean]` はこれ。
//!    `crates/typer/src/symbol.rs` の `Applied` 対 その他の規則。
//!
//! 2. **文脈境界の evidence の型が self type 越しに展開されない。**
//!    `[U : BCT]` は境界を裸の名前で書くので `tree_to_type` の
//!    「適用済み型」経路を通らず、`expand_type_members` が走らなかった。
//!    ケーキの中では本体の `implicitly[BCT[U]]`（self type で具体化される）
//!    と食い違い、唯一の候補が要求に合わなくなる。
//!    `Checker::expand_bound_evidence`。
//!
//! 3. **コンパニオン object の `protected` メンバ。** nsc の
//!    `accessWithin(ab) || accessWithinLinked(ab)`（`ab = sym.owner`）は、
//!    所有者の中か **そのコンパニオンの中** にいれば protected でも通す。
//!    サブクラス規則しか見ていなかったので、slick の
//!    `object ResultConverterCompiler { protected lazy val logger }` を
//!    同名 trait から読めなかった。
//!
//! 4. **入れ子の `private[pkg] object` / `class`。** `namer_enter_tmpl` が
//!    `ClassDef` / `ModuleDef` の `private_within` を記録していなかったので、
//!    修飾付き private が素の private として扱われていた。slick の
//!    `GetResult.GetUpdateValue`（`private[jdbc] object`）。
//!    ブリーフの「コンパニオンの private」という見立ては誤りで、これは
//!    修飾付き private の取りこぼし。
//!
//! 5. **匿名クラスの self alias。** `new T { base => … }` の `base` を
//!    パーサが捨てていた（`parse_new` が `self_name: None` 固定）。
//!    slick `TableQuery` の `not found: value base`。
//!
//! 6. **構成子パターンの関数位置。** nsc は `typingConstructorPattern` では
//!    非 stable なメソッドを名前解決の候補から外す。slick の `Node` は
//!    `final def :@ (newType: Type)` を持ち、抽出子 `object :@` は
//!    `TypeUtil._` から import されている。メソッドが抽出子を隠して
//!    `not found: extractor :@` になっていた。`SymbolTable::lookup_extractor`。
//!
//! 7. **Java の `Object` 戻り値。** nsc の `objToAny` は Java メソッドの
//!    *引数* だけを `Any` に広げる。戻り値・フィールド・型引数は `AnyRef`。
//!    全部 `Any` にしていたので `cv.unwrapped eq null` が
//!    `value eq is not a member of Any` になっていた。
//!
//! 8. **`scala.collection.Map` のメンバ。** `prelude_hier` の作るリンク用
//!    トレイトはメンバを持たず、`get`/`contains`/`getOrElse`/`apply` は
//!    `immutable.Map` / `mutable.Map` の側にしかなかった。抽象側の型で
//!    受けた slick `ExpandTables` が `value contains is not a member of Map`
//!    になっていた。`crates/typer/src/prelude_implfind.rs`。
//!
//! さらに、これらの副産物として `Ref.Make[F]` のような **pickle 由来の
//! 入れ子クラス** が型位置でコンパニオン object に解決されて
//! 「`Make` does not take type parameters」になっていたのも直した
//! （`Checker::lookup_qualified_type`）。
//!
//! `implfind.scala` は scala-library モード専用（私有ランタイムに `Map` が
//! ない）。`implfind_bad.scala` は 3 と 4 で緩めたアクセス規則の裏側で、
//! nsc も拒否する形が今も拒否されることを見る。

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-implfind-{tag}-{}-{nanos}-{seq}",
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
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn run_java_verified(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_fails_with(name: &str, needles: &[&str], extra: &[&str]) {
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
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {err:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// 8 ケースまとめて型検査し、nsc 2.13.16 と同じ標準出力を出すこと。
#[test]
fn fixtures_implfind_scala_library() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip implfind: scala-library jar not obtainable");
        return;
    };
    let out = compile_fixture_with("implfind", &["--scala-library", jar.to_str().unwrap()]);
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        let got = run_java_verified(&cp);
        assert_eq!(got, expected_stdout("implfind"), "stdout mismatch");
    }
    let _ = fs::remove_dir_all(&out);
}

/// 緩めた 2 つのアクセス規則が緩みすぎていないこと。nsc もこの 2 件を拒否する。
#[test]
fn fixtures_implfind_bad_is_error() {
    compile_fails_with(
        "implfind_bad",
        &[
            "value hidden cannot be accessed as a member of Prot$ from Stranger",
            "value Inner cannot be accessed as a member of Outer$ from Outsider",
        ],
        &["--no-scala-library"],
    );
}
