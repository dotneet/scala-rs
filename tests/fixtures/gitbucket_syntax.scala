// Syntax gitbucket's sources and its Twirl-generated templates need, all of
// which used to be parse errors. See docs/gitbucket.md.
object Main {
  // gitbucket writes `Database() withTransaction { implicit session => ... }`
  // fifteen times: a block whose only statement is a function literal with an
  // implicit parameter (nsc `blockStatSeq`).
  def withSession[A](f: String => A): A = f("session")

  def add(x: Int)(implicit b: Int): Int = x + b

  // `-` directly before a numeric literal is part of the literal, not an
  // identifier pattern (nsc `simplePattern`).
  def classify(i: Int): String = i match {
    case -1 => "minus one"
    case -2 => "minus two"
    case 0  => "zero"
    case _  => "other"
  }

  def main(args: Array[String]): Unit = {
    val n = withSession { implicit session =>
      val len = session.length
      len + 1
    }
    println(n)
    // `implicit` followed by a definition keyword is still a local definition.
    implicit val bump: Int = 10
    println(add(1))
    println(classify(-1))
    println(classify(-2))
    println(classify(0))
    println(classify(7))
    // A comment that starts right after an operator ends the operator (nsc
    // `getOperatorRest`), so this is `=>` and a comment, not the operator
    // `=>/*`. Twirl emits `case _ =>/*75.22*/ {` in every template.
    val opt: Option[Int] = Some(3)
    val s = opt.map/*1.1*/ {/*1.2*/ case v =>/*1.3*/ "v=" + v }.getOrElse/*1.4*/ ("none")
    println(s)
    val u = n +/*1.5*/ 1
    println(u)
  }
}
