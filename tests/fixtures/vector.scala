object Main {
  def main(args: Array[String]): Unit = {
    val v = Vector(1, 2, 3)
    println(v.apply(0))
    println(v.apply(1))
    println(v.apply(2))
    println(v.length)
    v.foreach(x => println(x))
  }
}
