object Main {
  def main(args: Array[String]): Unit = {
    println("ab" ++ "cd")
    println("abc".lengthIs)
    println("abc".sizeIs)
    println("ab".flatMap((c: Char) => c.toString + c.toString))
  }
}
