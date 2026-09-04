// `--no-scala-library` (the private runtime) carries no implementation of
// `Predef.genericWrapArray` / `copyArrayToImmutableIndexedSeq`, so an `Array` must
// not go through as a `Seq`. Emit a diagnostic rather than accepting it quietly
// (`.agent-brief.md`, "no stubs").
object Main {
  def v(a: Array[Any]): Iterable[Any] = a
  def main(args: Array[String]): Unit = ()
}
