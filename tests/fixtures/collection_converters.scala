import scala.jdk.CollectionConverters._
object Main {
  def show(x: Any): Unit = println(x)
  def main(args: Array[String]): Unit = {
    val jl = new java.util.ArrayList[Int]()
    jl.add(41)
    val buf = jl.asScala
    show(buf.head)
    val xs = List(1, 2)
    val jlist = xs.asJava
    show(jlist.get(1))
  }
}
