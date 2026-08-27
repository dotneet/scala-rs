object Main {
  def main(args: Array[String]): Unit = {
    val ps = "a,b,c".split(",")
    println(ps(0))
    println(ps(1))
    println(ps(2))
    println("abcde".diff("bd"))
    println("abcde".intersect("cde"))
  }
}
