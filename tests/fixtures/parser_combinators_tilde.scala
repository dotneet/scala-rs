package pcg

import scala.util.parsing.combinator._

// `~` is both a member class of `trait Parsers` (`case class ~[+a, +b]`,
// class file `Parsers$$tilde`) and the method that builds it; a grammar
// names the class as a type, as an extractor and as a constructor.
object Calc extends RegexParsers {
  def num: Parser[Int] = """-?\d+""".r ^^ (_.toInt)
  def sum: Parser[Int] = (num ~ ("+" | "-") ~ num) ^^ { case a ~ op ~ b => if (op == "+") a + b else a - b }
  def factor: Parser[Int] = num | literal("(") ~> expr <~ literal(")")
  def term: Parser[Int] = factor ~ rep("*" ~ factor) ^^ { case f ~ fs => fs.foldLeft(f) { case (acc, _ ~ x) => acc * x } }
  def expr: Parser[Int] = term ~ rep(("+" | "-") ~ term) ^^ {
    case t ~ ts => ts.foldLeft(t) {
      case (acc, "+" ~ x) => acc + x
      case (acc, _ ~ x) => acc - x
    }
  }
  def pair: Parser[String ~ Int] = "[a-z]+".r ~ num
  def all(s: String): Int = parseAll(sum, s).getOrElse(-1)
  def eval(s: String): String = parseAll(expr, s).map(v => s"ok $v").getOrElse("fail")
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Calc.all("1+2"))
    println(Calc.all("7-10"))
    println(Calc.all("7*10"))
    println(Calc.eval("2*(3+4)-5"))
    println(Calc.eval("2*"))
    val p = Calc.parseAll(Calc.pair, "abc42").get
    val built = new Calc.~("x", 1)
    val Calc.~(s, n) = p
    println(s"${p._1} ${p._2} $built ${p == Calc.~("abc", 42)} $s$n")
  }
}
