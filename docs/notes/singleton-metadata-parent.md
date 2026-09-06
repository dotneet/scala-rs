# Singleton の分割コンパイルとマクロ受信側

`Singleton` は JVM 上では Object に消去されるが、ScalaSignature には
`scala.Singleton` として保存する必要がある。読み取り側で組み込み記号に
復元し、書き出し側でも JVM 名から `java.lang.Singleton` を作らないようにする。

初期クラスパス走査が作る簡略なメソッド情報には型パラメータの境界がない。
そのクラスを使うときは既存の完全な pickle 読み込みで更新する。親クラスの
メンバーを継承スコープへ登録する時点でも更新し、古いメソッド記号を
スコープに残さない。ソース定義と手書き prelude は更新対象から除く。

マクロの `c.universe.WeakTypeTag` は受信側を評価して取得する内部オブジェクト。
明示された修飾式が目的のオブジェクトを返す場合はその式を生成し、
外部クラスの所有者情報が不完全でも静的 `MODULE$` 読み出しへ置き換えない。

`singleton_metadata` テストは provider / consumer の各側を scalac 2.13.16 と
scala-rs でコンパイルする全4組合せを検査する。正常なオーバーライドの実行は
`java -Xverify:all` で `7` と `bound` を出力し、型境界を不正に狭めた
オーバーライドは拒否する。既存の engine テストはマクロの展開と実行を検査する。

作業証跡: `/tmp/scala-rs-codex/integration/reify-audit-e521/`。
`bounds-roundtrip.log` は全4組合せの成功を記録する。正常系はすべて実行し、
異常系は終了コードに加えてオーバーライド診断を確認する。
`roundtrip-focused.log` の engine 27件と Singleton 18ケースは成功したが、
同じログの metadata テストは境界専用の書き出し経路を直す前の失敗である。
成功証跡と混同しない。`roundtrip-measures/measures` の4ライブラリ計測は
修正前の候補と同じ診断数だった。この計測も最後の境界書き出し修正より前である。
このノートは全体検証や main への受け入れ完了を意味しない。
