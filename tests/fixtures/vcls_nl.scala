object Main {
  // nsc `inLastOfStat`/`inFirstOfStat`: `}` can end a statement and `-` can
  // begin one, so the line break between them separates.
  def afterBlock: Int = {
    val x = { 1 }
    -1
  }

  def afterIf: Int = {
    if (true) { 1 }
    -2
  }

  def afterParen: Int = {
    val y = (1)
    -3
  }

  // A bare identifier line followed by `- 1` is two statements too.
  def twoStats: Int = {
    val b = 10
    b
    - 4
  }

  // The operator at the end of the line keeps the expression going.
  def trailingOp: Int = 1 +
    -2

  // Inside parentheses a line break never separates.
  def inParens: Int = {
    val c = 5
    (c
    - 1)
  }

  def statementIf(flag: Boolean): String = {
    val buf = new StringBuilder
    if (flag) { buf.append("y") }
    flag match {
      case true => 1
      case false => ()
    }
    buf.toString
  }

  def main(args: Array[String]): Unit = {
    println(afterBlock)
    println(afterIf)
    println(afterParen)
    println(twoStats)
    println(trailingOp)
    println(inParens)
    println(statementIf(true))
    println(statementIf(false))
  }
}
