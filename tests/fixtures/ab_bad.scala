// Boxing the value of a block does not make the block's own type go away: a
// `String` block still does not implement `def next(): Int`, and erasure must
// not paper over the mismatch on its way to `next()Ljava/lang/Object;`.
object Main {
  trait It[A] { def next(): A }

  def main(args: Array[String]): Unit = {
    val j = new It[Int] { def next(): Int = { val z = "y"; z } }
    println(j.next())
  }
}
