// Macro implementations whose `reify { … }` bodies are blocks, and whose
// bodies name members of static `object`s (`docs/macros.md` §7.17).
//
// Compiled first; `rf_use.scala` is compiled against this one, which is what
// makes each `reify` really expand and really run. This is the shape
// `test/files/run/macro-reify-basic` in the scala/scala corpus is written in.
import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

object RfHelper {
  def twice(i: Int): Int = i * 2
  val four = 4
  def shout(s: String): String = s + "!"
}

object RfImpl {
  // `println` is a member of `scala.Predef`, not an `object` of its own.
  def greet(c: Context)(s: c.Expr[String]): c.Expr[Unit] =
    c.universe.reify { println("hello " + s.splice) }

  // A block: two statements and no value.
  def twoLines(c: Context)(s: c.Expr[String]): c.Expr[Unit] =
    c.universe.reify { println(s.splice); println(RfHelper.twice(21)) }

  // A block whose last expression is the value, with a splice used twice --
  // if the tree dropped one or built it twice the printed side effects would
  // not match.
  def report(c: Context)(s: c.Expr[String]): c.Expr[String] = {
    import c.universe._
    reify { println(s.splice); println(RfHelper.four); RfHelper.shout(s.splice) }
  }
}
