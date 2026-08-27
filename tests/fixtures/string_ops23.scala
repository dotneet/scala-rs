object Main {
  def main(args: Array[String]): Unit = {
    "ab".iterator.foreach(c => println(c))
    println("ab".sizeCompare(3))
    println("ab".sizeCompare(2))
    println("ab".knownSize)
    println("ab".appendedAll("cd"))
    println("ab".prependedAll("xy"))
  }
}
