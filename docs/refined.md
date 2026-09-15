# refined の coreJVM ビルド

## 対象と確認範囲

[fthomas/refined](https://github.com/fthomas/refined/tree/11560e094e8cb4ea5b6c09a9f72572f179cc80b9)
のコミット `11560e094e8cb4ea5b6c09a9f72572f179cc80b9` を、Scala 2.13.18、
shapeless 2.3.13 で確認した。対象は `coreJVM / Compile` の45ソース
（手書き44個と sbt-buildinfo が生成する `BuildInfo.scala`）。
ソースの改変・除外・スタブ化は行っていない。

公式 `sbt ++2.13.18 coreJVM/compile` が選択した同じソースと6個の依存 jar を
scala-rs に渡すと、空の出力先に360個のクラスファイルを生成できる。
検証スクリプトはこれらを `refined_2.13-scala-rs.jar` にまとめる。
変更前の scala-rs `ee2e3869` は同じ条件で222エラーだった。

生成した refined の JAR に対して、scala-rs と公式 scalac のそれぞれで
[Smoke.scala](../tests/refinedrun/Smoke.scala) をコンパイルし、`java -Xverify:all`
で以下を確認する。利用側のクラスパスに公式ビルドの refined クラスは含めない。

- `Month.from` の範囲内・範囲外判定。
- `NonEmptyString.from` の空文字列判定。
- `MD5.from` の長さと16進文字の判定。
- `Adjacent` の Double / Float の隣接値。

このスクリプトは coreJVM のソースコンパイルと代表的な JVM 実行を確認する。
coreJVM の全528テストは末尾の別検証で確認した。追加連携モジュール、
Scala.js、Scala Native、Scala 3 は今回の対象外。

## 対応したコンパイラ機能

1. `class Impl(val c: Context)` 形式の macro bundle の宣言、ScalaSignature、
   実行時のインスタンス生成。公式 scalac との相互分割コンパイルも確認する。
2. refined receiver 上の型別名、バイナリの抽象型メンバー、暗黙引数からの推論、
   リテラル singleton 型の保持。shapeless の `Witness.Aux` と `ToList` が使う
   メンバーを、消去された JVM 型に置き換えずに扱う。
3. `Dynamic` による型の接頭辞のマクロ展開と、`c.parse` の正常なソース断片。
   `c.typecheck` が返す定数型と型メンバーを含む refinement をマクロに渡し、
   属性付きの型キャリアを受け取る。高階型別名の前方参照にも対応する。
4. 型引数を持つ `this` の reify、期待型を用いる reify 本体の型検査、
   `q"${lit: Literal}"` の型付き Tree パターン。
5. Scala の演算子名を含むバイナリクラスの所有者、Regex の実際のコンストラクタ、
   完全修飾された Java static メソッドの関数値への変換。

`c.parse` の異常系で checked `ParseException` を捕捉して回復する処理は、
現在の Context proxy では未対応として診断する。任意の Context API や
マクロが構築する任意の型・Tree に対応したという意味ではない。

## 再現手順

JDK、sbt、Python 3.9 以降、release ビルドした scala-rs を用意する。
初回の sbt 実行には依存物のダウンロードが必要。

```sh
git clone https://github.com/fthomas/refined.git /tmp/refined-source
git -C /tmp/refined-source checkout 11560e094e8cb4ea5b6c09a9f72572f179cc80b9
cd /tmp/refined-source
sbt -batch -Dsbt.supershell=false '++2.13.18' 'coreJVM/compile' \
  'show coreJVM/Compile/sources' 'show coreJVM/Compile/dependencyClasspath' \
  > /tmp/refined-sbt.log 2>&1
```

scala-rs リポジトリのルートで実行する。`--output` はまだ存在しないディレクトリを
指定する。古い classfile を再利用しないよう、スクリプトが既存の出力先を拒否する。

```sh
cargo build --release -p scala-rs-cli
python3 tests/refined_check.py \
  --checkout /tmp/refined-source \
  --sbt-log /tmp/refined-sbt.log \
  --output /tmp/refined-result
```

`result.json` にソース数、クラス数、各コマンドと終了コードを記録する。
両方の JVM 実行が `REFINED_SMOKE_PASS` を出力した場合だけ `verdict: PASS` を
記録する。これは `tests/verify_merge.sh` の全互換性ゲートとは別の検査。

scala-rs には `-feature`、`-unchecked`、必要な `-language`、
`-Xfatal-warnings` を渡す。nsc 専用の lint / unused オプションと `-release 8`
は渡していないため、公式 sbt の全警告設定の再現を検証するものではない。

関連する小規模な回帰テストは `aliaslookup`、`macrotransportbatch`、`reify2`、
`czero` に置いてある。受理・拒否と実行結果を公式 Scala 2.13.16 と比較する。
継承した型別名の境界、抽象型を接頭辞に持つ型、HList、case class の抽出パターンは、
`erascg`、`gzero`、`slickshape`、`warn` でも確認する。

scala-rs の workspace 全体を検査する場合は、基準と同じ JDK 17 を
`JAVA_HOME` と `PATH` に設定して、次を実行する。今回の確認環境は
Temurin 17.0.3。数値の文字列表現や Unicode の識別子判定を比較するテストは
JDK のバージョンの影響を受ける。

```sh
WT_DIR=/tmp/scala-rs-workspace-result WT_JOBS=6 WT_THREADS=4 tests/workspace_tests.sh
cargo fmt --all --check
cargo clippy --workspace --release --all-targets
```

workspace runner の最後に `missing=0 failed_bins=0` が出ることを確認する。

## 今回の検証結果（2026-09-16）

- coreJVM: 45ソース、360クラス、JAR 生成成功。上流ソースの変更なし。
- 生成 JAR に対する公式 scalac / scala-rs の利用側コンパイルと
  `java -Xverify:all` の実行: 両方 `REFINED_SMOKE_PASS`。
- workspace 全体: 3,436件成功、失敗・ignored ともに0。
  `binaries=24 rows=31 missing=0 failed_bins=0 doc_rows=7` を確認。
  下記のテストスイート修正後にも全件を再実行して同じ結果を確認した。
- `cargo fmt --all --check` 成功。Clippy 成功、今回の変更による新規警告なし
  （既存の警告は残る）。

## refined 自身のテストスイート（2026-09-16 修正後）

上記の3,436件は scala-rs 自身の回帰テストであり、refined のテストではない。
refined の `coreJVM/test`（Scala 2.13.18、全35テストソース）の追試結果は次のとおり。

| 確認条件 | 修正前 | 修正後 |
|---|---|---|
| 公式 scalac / sbt（対照） | 528件成功 | 同じ対照結果を使用 |
| 公式コンパイル済みテストを scala-rs 生成JARで実行 | 525件成功、3件エラー | 528件成功 |
| scala-rs 生成JARに対してテストを公式 scalac で再コンパイル | 34エラー | エラー・警告0件 |
| 上の再コンパイルしたテストを scala-rs 生成JARで実行 | コンパイル失敗のため実行不可 | 528件成功、失敗・エラー0件 |
| scala-rs 生成JARに対してテストを scala-rs でコンパイル | 1,078エラー | 1,084エラー、実行不可 |

実行時の3件は `Regex.isValid`、`Regex.showExpr`、`Regex.showResult`。
以下の原因を修正した。上流ソースとテストの変更・除外は行っていない。

1. 明示的な case class companion の対応先を単純名だけで探していた。
   `Regex` が別パッケージのクラスを選び、`apply()` / `unapply` が生成されなかった。
   所有者と名前を組にしてクラスを解決する。
2. ScalaSignature の入れ子のワイルドカードを外側に引き上げていた。
   `T => Iterable[_]` と `ToList[RT, Result[_]]` の型引数内の存在型を保ち、
   文字列の `Validate` や `OneOf` の暗黙値探索を復旧した。
3. shapeless の `::` を Scala の List の `::` として保存していた。
   シンボルの実際の所属先を使い、`AllOf` / `AnyOf` などの型情報を修正した。
4. オブジェクトの型情報に所属パッケージの接頭辞が欠けていた。
   完全修飾名を復旧し、`illTyped` が期待する診断文字列との不一致を解消した。
5. 値クラスに生成する `equals` / `hashCode` とその `$extension` メソッドを
   型情報に記録していなかった。両方を宣言し、公式 scalac の誤った等価比較警告と
   extension メソッドの解決失敗を防いだ。

テストは公式 sbt が選ぶ全35ソース（生成 doctest 7ソースを含む）を使用する。
直接コンパイルには Scala 2.13.18 と同じ依存JARを使い、`-feature`、`-unchecked`、
必要な `-language`、`-Xfatal-warnings` を有効にした。
実行は sbt セッション内で `coreJVM / Test / fullClasspath` の
`Compile / classDirectory` を scala-rs の JAR に、`Test / classDirectory` を
再コンパイル先に置換して `coreJVM/test` を実行した。
表示したクラスパスに公式 refined 本体と古いテストの出力先が含まれず、
`Passed: Total 528, Failed 0, Errors 0, Passed 528` が出ることを確認した。

テスト自体を scala-rs でコンパイルする場合は未対応部分が残る。
ScalaCheck の演算子への暗黙変換、依存型の推論、マクロの型・Tree の受け渡し、
`scala.tools.nsc.Global` に直接依存するマクロなどで診断が出る。
1,084件は連鎖的な診断を含み、独立した不具合の数ではない。
`::` の型情報を直して HList の暗黙値探索が進むと、その先の診断が増えた。
したがって、528件成功は「本体を scala-rs、テストを公式 scalac で生成した」結果であり、
テストまで scala-rs で生成できたという意味ではない。
