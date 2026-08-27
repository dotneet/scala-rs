object Main {
  def main(args: Array[String]): Unit = {
    println("b".compare("a"))
    println("a".compare("b"))
    println("ab".lengthCompare(3))
    println("ab".lengthCompare(2))
    println("abcdef".patch(2, "XY", 2))
    println("a" < "b")
    println("b" < "a")
  }
}
