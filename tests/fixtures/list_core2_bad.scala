// `sorted` is a default method on scala-library's `SeqOps` and is absent from the
// private runtime's `List` classfile. Under `--no-scala-library` emit a diagnostic
// rather than accepting it quietly.
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 3 :: 1 :: 2 :: Nil
    println(xs.sorted)
  }
}
