object Main {
  object Curried {
    def value(n: Int)(implicit delta: Int): Int = n + delta
  }

  def main(args: Array[String]): Unit = {
    println(org.scalatest.Assertions.identity(7))
    val n = 7
    println(org.scalatest.Assertions.withClue(n, s"value=$n"))
    println(org.scalatest.Assertions.identity((1, 2) match {
      case (a, b) => a + b
    }))
    println(org.scalatest.Assertions.typedMatch())
    implicit val delta: Int = 4
    println(org.scalatest.Assertions.retype(Curried.value(7)))
    println(org.scalatest.Assertions.retypeArray(Array("aa", "bb")).length)
  }
}
