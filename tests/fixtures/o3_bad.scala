// `getOrElse[B >: A]` widens to the *lub*; it does not keep the receiver's
// element type. `Option[Int].getOrElse("no")` is an `Any`, so assigning it to
// an `Int` is an error -- exactly as scalac 2.13.16 reports it.
object Main {
  def main(args: Array[String]): Unit = {
    val o: Option[Int] = Some(1)
    val n: Int = o.getOrElse("no")
    println(n)
  }
}
