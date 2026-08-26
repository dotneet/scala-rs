object Main {
  def main(args: Array[String]): Unit = {
    val v0 = Vector.empty
    val v = v0.:+(1).:+(2)
    println(v.apply(0))
    println(v.apply(1))
    println(v.length)
    v.foreach(x => println(x))
  }
}
