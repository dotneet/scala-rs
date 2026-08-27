object Main {
  def main(args: Array[String]): Unit = {
    val b = new scala.collection.mutable.StringBuilder()
    b += 'a'
    b.append("bc")
    println(b.toString)
  }
}
