object Main {
  def main(args: Array[String]): Unit = {
    val a = Array(1, 2, 3, 2)
    println(a.find(_ == 2))
    println(a.find(_ == 9))
    println(a.contains(2))
    println(a.contains(9))
    a.distinct.foreach(x => println(x))
    a.takeRight(2).foreach(x => println(x))
    a.dropRight(2).foreach(x => println(x))
    a.takeWhile(_ < 3).foreach(x => println(x))
    a.indices.foreach(i => println(i))
    println(a.lengthCompare(4))
    println(a.lengthCompare(3))
  }
}
