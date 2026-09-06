# 直接自己末尾呼び出し

`final` / `private` メソッド、object のメソッド、持ち上げ後のローカル def の
直接自己末尾呼び出しを JVM の後方分岐へ変換する。`@tailrec` がある場合は
既存の型検査による実効的 final 判定（sealed class なども含む）を利用する。
相互再帰は対象外。

## 意味論

`gen_tailrec.rs` は型消去後の本体から `if` の枝、`match` の各本体、block の
最後の式、型注釈の式にある自己呼び出しを選択する。引数、条件、guard、
ネストした定義、try/finally の内部は末尾位置として扱わない。

通常の呼び出しと同じコード生成で receiver と全引数を左から順に評価し、
その後 JVM の引数スロットへ逆順に格納する。これで引数の交換、Long/Double の
2 スロット値、Unit、型消去による box/unbox、持ち上げた捕捉引数を扱う。
別 receiver への呼び出しでは、引数評価後に slot 0 を更新する。
scalac の変換と同様に、追加の null 検査は入れない。`TrcNull.hop(2, null)` は
null を一度 receiver にしても本体がフィールドを読まず次の周回で元の instance に戻るため
正常終了する。通常の `invokevirtual` の null receiver とは異なる、実測した nsc の挙動である。
ループ先は捕捉フィールドのロードより前なので、receiver の変更後に捕捉値も更新する。

by-name パラメータを別の by-name 引数へそのまま渡す場合、typer は既存の thunk を
引き継ぐ。毎周 `() => x` と包装すると、本体がループになっても最終の値評価で
thunk の鎖を再帰してしまうためである。

注釈付きメソッドで、型消去後の未対応形状や処理できなかった末尾呼び出しが残る場合は
コンパイルエラーにする。特に value class の `$extension` メソッドの尾再帰は未対応。
明示的な `return` 内の自己再帰、try/catch/finally 内の再帰は現行の型検査が拒否する。
これらを Scala 2.13 互換として扱ったことにはしない。

## 回帰テスト

```sh
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test -p scala-rs-cli --release --test trc_tailrec
```

`tests/fixtures/trc_deep.scala` は 100 万～200 万回の再帰を `-Xss256k -Xverify:all`
で実行し、scalac 2.13.16 との出力一致を検査する。wide 引数交換、match/block、
local def、可変変数の捕捉、receiver 交換、引数の副作用順、curried、generic、Unit、
by-name、注釈なしの final、引数なしメソッドを含む。`javap -p -c` で各対象メソッドから
再帰呼び出しが消え、分岐が生成されていることも検査する。

`trc_client.scala` は scalac が scala-rs の classfile を参照して別コンパイルする
相互運用テスト。`trc_bad.scala` と `trc_inputs_bad.scala` は override 可能なメソッド、
非末尾再帰、receiver 内の再帰、前の引数節内の再帰を両コンパイラが拒否することを検査する。
`trc_valueclass_unsupported.scala` は scalac が受理する合法なプログラムであり、
こちらの未対応を明示するためのテスト。Scala の不正プログラムを表す負例ではない。

## Zulu 15.0.6 の JIT による比較の落とし穴

この開発環境の既定 Java は次の版であった。

```
openjdk version "15.0.6" 2022-01-18
OpenJDK Runtime Environment Zulu15.38+17-CA (build 15.0.6+5-MTS)
OpenJDK 64-Bit Server VM Zulu15.38+17-CA (build 15.0.6+5-MTS, mixed mode)
```

`TrcDeep.matching(2000000, 0)` は 2000000 を返すべきだが、この VM の既定 JIT では
実行ごとに異なる小さい値になった。**scalac 2.13.16 が出力した同じプログラムでも再現**
している。scala-rs の出力だけを見てコンパイラの不正変換と判断してはならない。

```sh
# <out> は scala-rs または scalac が trc_deep.scala をコンパイルしたディレクトリ
java -Xverify:all -Xss256k -cp <out>:/tmp/scala-rs-lib/scala-library-2.13.16.jar TrcDeep
# 同じ classfile がこちらでは期待値を返す
java -Xint -Xverify:all -Xss256k -cp <out>:/tmp/scala-rs-lib/scala-library-2.13.16.jar TrcDeep
java -XX:TieredStopAtLevel=1 -Xss256k -cp <out>:/tmp/scala-rs-lib/scala-library-2.13.16.jar TrcDeep
```

Temurin 17.0.3 の既定 JIT でも両コンパイラの出力が正しく動くことを確認した。
回帰テストは環境にある Temurin 17 を優先し、存在しない環境では Java の `-Xint` を使う。
JIT の内部原因を特定したという主張ではない。
