object Main {
  def main(args: Array[String]): Unit = {
    println("hello".toUpperCase)
    println("HELLO".toLowerCase)
    println("foobar".stripPrefix("foo"))
    val ps = "a,b".split(',')
    println(ps.apply(0))
    println(ps.apply(1))
  }
}
