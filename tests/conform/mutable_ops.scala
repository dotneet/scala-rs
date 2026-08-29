import scala.collection.mutable
object Main {
  def main(args: Array[String]): Unit = {
    val hm = mutable.HashMap[String, Int]()
    hm += ("a" -> 1)
    hm ++= List("b" -> 2, "c" -> 3)
    hm -= "a"
    println(hm.size)
    val hs = mutable.HashSet[Int]()
    hs ++= List(1, 2, 3)
    hs -= 2
    println(hs.size)
    val ab = mutable.ArrayBuffer[Int]()
    ab ++= List(4, 5)
    println(ab.size)
  }
}
