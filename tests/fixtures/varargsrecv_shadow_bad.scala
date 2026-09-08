// A repeated parameter does not follow a program's own binding of the name
// `Seq`. scalac 2.13.16: `value tag is not a member of Seq[Int]`.
//
// This one is not merely a message. `gen_desc` writes
// `Lscala/collection/immutable/Seq;` for a repeated parameter whatever the
// typer decided, so accepting it emitted `invokevirtual Main$Seq.tag` against
// an `ArraySeq$ofInt`: an unmodified build of the branch point compiles this
// file and dies at run time with
// `ClassCastException: class scala.collection.immutable.ArraySeq$ofInt cannot
// be cast to class Main$Seq`.
object Main {
  class Seq[A] { def tag: String = "MINE" }
  def f(xs: Int*): String = xs.tag
  def main(args: Array[String]): Unit = println(f(1, 2, 3))
}
