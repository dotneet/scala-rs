// `--no-scala-library`（私有ランタイム）には `Predef.genericWrapArray` /
// `copyArrayToImmutableIndexedSeq` の実体が無いので、`Array` を `Seq` として
// 通してはならない。黙って通さず診断を出すこと（`.agent-brief.md`「スタブ禁止」）。
object Main {
  def v(a: Array[Any]): Iterable[Any] = a
  def main(args: Array[String]): Unit = ()
}
