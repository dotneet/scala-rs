object Main {
  def main(args: Array[String]): Unit = {
    val a = Array(1, 2, 3)
    a.zipWithIndex.foreach(t => { println(t._1); println(t._2) })
    println(a.knownSize)
    println(a.sizeCompare(4))
    println(a.sizeCompare(3))
    println(a.sizeCompare(2))
  }
}
