// The rejecting side of `bt_companion.scala`: finding the companion must not
// make anything fit that the companion does not offer.
//
// Compiled against `bt_companion_lib.scala`'s class files. Real scalac
// 2.13.16 reports three errors here as well.
import btc._

object Main {
  def main(args: Array[String]): Unit = {
    // `apply[T](v: T): Holder[T]` -- the explicit type argument pins `T`.
    println(Holder[Int]("s"))
    // `Empty.apply[T]` takes no value parameters.
    println(Empty[Int](3))
    // The companion has no such member.
    println(Holder.missing)
  }
}
