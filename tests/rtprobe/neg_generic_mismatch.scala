// scalac: type mismatch; found List[String], required List[Int].
object Main {
  def main(args: Array[String]): Unit = { val xs: List[Int] = List("a"); println(xs) }
}
