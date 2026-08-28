object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(3, 1, 4, 1, 5)
    println(xs.filter(x => x > 2).mkString(","))
    println(xs.filterNot(x => x > 2).mkString(","))
    println(xs.take(2).mkString(","))
    println(xs.drop(2).mkString(","))
    println(xs.takeRight(2).mkString(","))
    println(xs.dropRight(2).mkString(","))
    println(xs.takeWhile(x => x > 2).mkString(","))
    println(xs.dropWhile(x => x > 2).mkString(","))
    println(xs.slice(1, 4).mkString(","))
    println(xs.reverse.mkString(","))
    println(xs.distinct.mkString(","))
    println(xs.init.mkString(","))
    println(xs.tail.mkString(","))
    println(xs.toList.mkString(","))
  }
}
