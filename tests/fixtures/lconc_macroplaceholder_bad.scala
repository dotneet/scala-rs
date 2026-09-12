// A `macro ???` placeholder this compiler has no fast-track entry for is
// uncallable: nsc reports "macro implementation is missing" at the call site,
// and no implementation is loaded or run.
import scala.language.experimental.macros

object Main {
  def foo(x: Int): String = macro ???

  def main(args: Array[String]): Unit = println(foo(1))
}
