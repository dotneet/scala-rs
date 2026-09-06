# 外部定義の universal メソッドの上書き判定

候補 e9e17219 の全体テストで `inherited_binary_members_match_scalac` が失敗した。
完全な pickle を読み込んだ `Ops.hashCode(): Int` と、手書き prelude の
`Any.hashCode: Int` が別候補として残り、`println(c.hashCode)` が未解決の
overload 型を持ったままコード生成へ進んでいた。JVM は Int を Object として
渡す箇所を VerifyError で拒否する。

`drop_overridden` は所有者の上下関係を Class 記号同士で比較していたが、
親の型には `Type::Any` / `AnyRef` / `AnyVal` が使われる。この3つの所有者を
対応する型表現で比較し、同じシグネチャの親メンバーを候補から除く。
単純名ではなく prelude の記号 ID で区別するため、同名のユーザークラスは
この変換の対象にならない。

既存の ifacebridge テストは、scalac で作った trait 群を継承するクラスと
匿名クラスの出力を比較し、共変な返り値の bridge の descriptor も検査する。
修正後の2テストは成功した。証跡は
`/tmp/scala-rs-codex/integration/inherited-boxing-audit/repaired.log`。
関連52テストも成功した（`related.log`）。厳密な JVM 検証の実行は
`strict.out` / `strict.err` に記録している。
ライブラリ計測は cats 346診断、gitbucket 895診断、Slick 0診断・1490クラス、
scala-library 1557診断。gitbucket から消えた4診断は、いずれも上書き後の
`toString` が未解決 overload として残ったことによる誤診断だった。
全体検証の受け入れは別途必要である。
