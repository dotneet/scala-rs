case class C[T](x: T)
case class CS(xs: C[_]*)
case class Tagged[A](tag: String, xs: A*)
object Main {
  def one(v: CS): String = v match {
    case CS() => "empty"
    case CS(C(5), rest @ _*) => "five:" + rest.size
    case CS(C(9)) => "nine"
    case _ => "other"
  }
  def two(v: Tagged[Int]): String = v match {
    case Tagged("x", 1, rest @ _*) => "x:" + rest.sum
    case Tagged("x") => "empty"
    case _ => "other"
  }
  def main(args: Array[String]): Unit = {
    println(one(CS()))
    println(one(CS(C(5), C("abc"))))
    println(one(CS(C(9))))
    println(one(CS(C(9), C(8))))
    println(two(Tagged("x", 1, 2, 3)))
    println(two(Tagged[Int]("x")))
    println(two(Tagged("y", 1)))
  }
}
