class B
object Main {
  def main(a: Array[String]): Unit = {
    println(classOf[String].getName)
    println(classOf[B].getName)
    println(classOf[Int].getName)
    println(classOf[Array[Int]].getName)
  }
}
