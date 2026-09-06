# Codegen候補の親セッションによる回帰修正（未受け入れ）

起点は `81454c19`。内側クラスの ClassProjection WIP は含まない。

## 型検査の繰り返しによる速度劣化

候補81454c19の全workspaceは2275 PASS / 0 FAIL（205 result rows、1118.45秒）。
Slickは0 errors / 1490 classes、catsは355 errors / 83 filesだった。
続くGitBucketコンパイラはCPU約98%で走り続けた。スタックを1秒採取し、
10分51秒時点で親がそのコンパイラPIDだけにSIGTERMを送った。
検証runnerはgitbucketのexit 2を記録してexit 1で停止した。
**この候補のGitBucketは未完了。残りのscalalib、strict検証、Slick実行、
特殊化ledger、コーパスもこの全体runでは未実施。再起動済みとは扱わない。**

`object Main { val x = missing.f(0).f(0)... }` で再現した。
12段で0.79秒、16段と20段は各5秒でタイムアウト。
`try_rewrite_dynamic_apply` が、既に型の付いたqualifierを再度typecheckしていた。
通常のselection側でも暫定Errorを再試行するため、入れ子ごとに同じ木を重複して
検査していた。DynamicかどうかのprobeではNoTypeのqualifierだけを型検査し、
それ以外は完成済みの型を使うようにした。通常のselection側の暫定Error再試行は維持。

修正後: 20段0.10秒、40段0.17秒、80段0.46秒。
実GitBucket計測は10.93秒で終了（906 errors / 112 files）。
件数減少だけを成果とは扱わない。constructor ambiguity等の新しい診断差分は
未監査であり、main基準912 / 111に対する受け入れ条件をまだ満たしていない。

永続テスト `qualifier_retry::erroneous_application_chain_does_not_repeat_dynamic_receiver_typing`
は80段の不正な式を20秒以内に拒否し、未定義名の診断が1件だけであることを要求。
この余裕のある期限は正常時の速度を競うためではなく、旧実装の爆発的な再検査を
有限時間で検出するため。既存のcross-unit正常/異常×ファイル順序×nsc/rs比較もPASS。

## Function subclassのtyped pattern

`A => B` と `Function1[A,B]` が異なる表現のまま、抽象型引数を別の規則で消去
していた。型パターン互換性の入口でFunction構文をFunctionNクラスに正規化し、
双方の引数を同じ規則で扱うようにした。catsの新しい4件の誤診断が消えた。

`function_subclass_pattern.scala` はConstant、Wrapped、Zeroとwildcardパターンを
実行してnscと比較する。さらにFunction1/Function0のInt戻り値に対して
String戻り値のfinal subclassを検査する不正な例は、両処理系で拒否される。

## 確認済みと残件

- Cargo: function_pattern 1、qualifier_retry 2、seqpat 20、e2eのdynamic 5、合計28 PASS。
- fresh release build成功、git diff --check成功。
- Slick: 184 files / 0 errors / 1490 classes（2.26秒）。
- cats: 339 / skip1 / 351 errors / 81 files（2.29秒）。
- GitBucket: 353 / skip1 / 906 errors / 112 files（10.93秒）。
- Scala library: 538 / 1620 errors / 171 files（2.01秒）。

catsにはmonadErrorのandThen ambiguityが新しく1件残る。
Scala libraryにはTNode/CNode/LNode、MutableBufferWrapper、Listの型パターンで
追加診断があり、FilePropのconstructor ambiguityが消えている。
不足する型情報の下で不適合と断定していないか、実際のsymbol/継承関係を調べること。
診断数を合わせるために型をAnyに広げたり、型パターン検査を一括で無効化しない。

上記が解決してから新しい候補の全体ゲートへ進む。現時点ではマージ不可。

証拠:
- `/tmp/scala-rs-codex/integration/candidate-81454c1/results.json`
- 同 `gitbucket-sample.txt`、`gitbucket-parent-interruption.json`、`cats-parent-diff.txt`
- `/tmp/scala-rs-codex/integration/error-retry-growth/results.json`
- 同 `reused-qualifier/results.json`、`focused.log`、`dynamic-focused.log`
- 同 `measures/results.json` と各 `*-baseline-diff.txt`。

## Applied List/Option/Some の強制解決を除去（親の継続修正）

`check_types::tree_to_type(AppliedTypeTree)` が、名前の最後の要素が
List/Option/Someなら修飾子やsource定義にかかわらずprelude symbolを返していた。
コメントにも「source定義へ解決するとlibraryのエラー数が増えるので維持」と
明記されていた。しかし `custom.List[Int]` までscalaのListになるため、これは
互換性を保つ処理ではない。3種類も通常のconstructor名解決・型適用へ通した。

修正前のfixtureでは `custom.List` / `custom.Option` のconstructorが見つからず、
`value` accessorも見つからなかった。修正後は、修飾付きの型注釈・import経由の
constructor・明示的なscala標準Listを併用し、nscと同じ `7/option/9/3` を出力する。
custom.List[Int]→custom.List[String]、custom.Option[Int]→custom.Option[String]
の不正代入は両処理系で型不一致として拒否される。strict JVM実行も成功。

関連テスト24 PASS: applied_collection_names 1、aliaslookup 2、function_pattern 1、
seqpat 20。import経由のconstructorを追加後もapplied_collection_namesを再実行しPASS。
型パターン検査を無効化する変更や、Anyへ置き換える変更はない。

追加計測（fresh release build、既定のJDK17/UTF-8）:

- cats 351 errors / 81 files（1.85秒）
- GitBucket 902 errors / 112 files（5.52秒）
- Slick 0 errors / 1490 classes（1.76秒）
- Scala library **1880 errors / 203 files**（1.55秒、直前1620 / 171）

増えた診断全部の妥当性は未監査。数値の悪化だけを理由に誤ったsymbolへの解決に
戻してはならないが、増分をすべて正しいと断定してもならない。
全workspaceと全コーパスを検証し、失った実際の互換動作を修正すること。
全体受け入れ前の候補であり、main基準値は更新しない。

追加証拠: `/tmp/scala-rs-codex/integration/applied-collection-names/` の
before.log、focused.log、import-focused.log、measures/の各ログとprevious-diff。

### 切り分けられた別の不足

一時的な型関係トレースを採取し、全デバッグコードは除去済み。
`error-retry-growth/pattern-trace.log` のPATREL行に以下を記録した。

- TNode等のscrutineeが `Named { name: "MainNode", args: [] }` のまま。
  同じコンテキストで `class_sym_of` は実際のMainNodeのsymbolを発見できる。
- MutableBufferWrapperのscrutinee `java.util.List` がscala immutable Listの
  prelude symbol #50になる。この引数はソースで `ju.List[A]` と書かれており、
  今回除去したAppliedTypeTreeの処理が修飾子を無視する問題に該当する。
  修正後、このMutableBufferWrapperの誤診断は消えている。
  別途、classpath.rsのdescriptor readerにも完全JVM名よりsimple nameを
  優先する処理を見つけた。ただし今回の診断の原因とは断定しない。
  同名のJavaクラスを使うprovider/consumerで別に再現を取る必要がある。
- source Listの型パターンもprelude #50（parents=[AnyRef]）を使っていた。
  今回の名前解決変更の直接の動機はこの経路とcustom fixtureである。

トレース時の計測はJDK環境を明示していないため数値比較には使わず、上の
環境を固定した追加計測を数値の証拠に使う。
