object Main {
  def main(args: Array[String]): Unit = {
    println("ab".map((c: Char) => if (c == 'a') 'A' else c))
    println("ab" :+ 'c')
    println('x' +: "yz")
  }
}
