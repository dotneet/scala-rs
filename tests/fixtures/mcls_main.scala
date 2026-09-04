// Compiled against `mcls_lib.scala`'s classfiles, never alongside its source.
//
// `mcls.Codes.:@` is deliberately not called here: a module's *nested*
// class-like members do not reach our pickle at all (the same gap makes
// `object Box { class Inner }` invisible to real scalac), so only the
// classfile *name* `mcls/Codes$$colon$at$.class` is asserted, by the test.
object Main {
  def main(args: Array[String]): Unit = {
    println(mcls.util.twice(21))
    println(mcls.util.greeting)
    println(new mcls.Meters(3).plus(4))
    println(new mcls.Wrap[Int](5).self)
  }
}
