import scala.jdk.CollectionConverters._
object Main {
  def main(args: Array[String]): Unit = {
    val jl = new java.util.ArrayList[Int]()
    jl.add(41)
    val buf = jl.asScala
    println(buf.head)
    val xs = List(1, 2)
    val jlist = xs.asJava
    println(jlist.get(1))
  }
}
