object Main {
  def upcast[A, B](x: A)(implicit ev: A <:< B): B = ev(x)

  def main(args: Array[String]): Unit = {
    println(upcast[String, Int]("nope"))
  }
}
