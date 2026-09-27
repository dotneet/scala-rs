package pcr

import scala.util.parsing.combinator._

// What a grammar does with its results: match them against `Success` and
// `Failure` (each both a case class and an object in `trait Parsers`, and
// `Success` also a three-argument method), test for `NoSuccess` and
// `Error` (the latter also `java.lang.Error`), and pass plain strings to
// the by-name combinators through the `literal` view.
object Calc extends RegexParsers {
  def num: Parser[Int] = """\d+""".r ^^ (_.toInt)
  def factor: Parser[Int] = num | "(" ~> expr <~ ")"
  def expr: Parser[Int] = factor ~ rep("+" ~> factor) ^^ { case f ~ fs => f + fs.sum }
  def word: Parser[Any] = num | "none"

  def eval(s: String): String = parseAll(expr, s) match {
    case Success(v, _) =>
      val doubled: Int = v * 2
      s"ok $v $doubled"
    case Failure(msg, next) => s"failure at ${next.pos.column}: $msg"
    case Error(msg, _) => s"error: $msg"
  }

  def kind(r: ParseResult[Int]): String = r match {
    case e: Error => "error " + e.msg
    case ns: NoSuccess => "no success " + ns.msg
    case s: Success[_] => "success " + s.result
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Calc.eval("1+(2+3)"))
    println(Calc.eval("(4)"))
    println(Calc.eval("1+"))
    println(Calc.parseAll(Calc.word, "none"))
    println(Calc.kind(Calc.parseAll(Calc.expr, "7")))
    println(Calc.kind(Calc.parseAll(Calc.expr, "x")))
    println(Calc.kind(Calc.Error("boom", null)))
    println(Plain.kind(Plain.parseAll(Plain.num, "x")))
  }
}

// No wildcard import: `Error` is offered only by `java.lang._` here, and
// the inherited `Parsers.Error` still wins.
object Plain extends scala.util.parsing.combinator.RegexParsers {
  def num: Parser[Int] = """\d+""".r ^^ (_.toInt)
  def kind(r: ParseResult[Int]): String = r match {
    case e: Error => "error " + e.msg
    case f: Failure => "failure " + f.msg
    case _ => "other"
  }
}
