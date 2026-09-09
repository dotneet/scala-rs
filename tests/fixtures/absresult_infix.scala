class Pair[A, B](val a: A, val b: B)
object Main {
  type ::[A, B] = Pair[A, B]
  def main(args: Array[String]): Unit = {
    val p: Int :: String :: Boolean = new Pair(7, new Pair("right", true))
    println(p.a)
    println(p.b.a)
    println(p.b.b)
  }
}
