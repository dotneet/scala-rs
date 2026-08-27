object Main {
  def main(args: Array[String]): Unit = {
    "abc".indices.foreach(i => println(i))
    println("a+".r.findFirstIn("xaa").get)
    println("a+".r.matches("aa"))
  }
}
