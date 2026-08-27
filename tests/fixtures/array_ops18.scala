object Main {
  def main(args: Array[String]): Unit = {
    val a = Array(1, 2, 3, 2)
    a.filterNot(_ == 2).foreach(x => println(x))
    println(a.headOption)
    println(a.take(0).headOption)
    println(a.lastOption)
    println(a.take(0).lastOption)
    val p = a.partition(_ < 3)
    p._1.foreach(x => println(x))
    p._2.foreach(x => println(x))
    val s = a.splitAt(2)
    s._1.foreach(x => println(x))
    s._2.foreach(x => println(x))
    val sp = a.span(_ < 3)
    sp._1.foreach(x => println(x))
    sp._2.foreach(x => println(x))
  }
}
