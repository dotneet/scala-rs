object PairSeq {
  def unapplySeq(n: Int): Option[List[Int]] = Some(n :: (n + 1) :: Nil)
}
case class Point(x: Int, y: Int)
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 1 :: 2 :: 3 :: Nil
    val s = xs match {
      case List(a, b, c) => a + b + c
      case _ => 0
    }
    println(s)
    val t = 10 match {
      case PairSeq(a, b) => a + b
      case _ => -1
    }
    println(t)
    val h = xs match {
      case List(a, rest @ _*) => a
      case _ => 0
    }
    println(h)
    val p = Point(3, 4) match {
      case Point(y = b, x = a) => a + b
      case _ => 0
    }
    println(p)
  }
}
