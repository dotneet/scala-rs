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
