// scala-rs rejects: `orNull[A1 >: A](implicit ev: Null <:< A1)` on
// Option[Int] (A1 is inferred as Any) and on None (A1 = Null).
object Main {
  def main(args: Array[String]): Unit = {
    val some: Option[Int] = Some(3)
    println(some.orNull)
    println(None.orNull)
  }
}
