import scala.collection.mutable
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(5, 3, 1, 4)
    println(xs.sorted)
    println(xs.filter(_ > 2))
    println(xs.map(_ * 2))
    println(xs.sum)
    println(xs.mkString("[", ",", "]"))
    println(xs.reverse.take(2))
    println(xs.zipWithIndex.map { case (v, i) => i.toString + ":" + v.toString }.mkString(" "))
    val m = mutable.Map[String, Int]()
    m("a") = 1
    m("b") = 2
    println(m.toList.sortBy(_._1))
    val im = Map("x" -> 1, "y" -> 2)
    println(im.getOrElse("x", 0))
    println(im.contains("z"))
    val buf = mutable.ArrayBuffer[Int]()
    for (i <- 1 to 3) buf += i
    println(buf.mkString("-"))
  }
}
