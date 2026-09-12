// A whitebox macro def whose implementation does not correspond to it: the
// implementation takes an argument the definition has no parameter for.
//
// Whitebox macros used to be refused outright at the binding, which short-cut
// every later check; now that they are bound like blackbox ones
// (`docs/macros.md`), the shape check is what has to reject this. Real scalac
// 2.13.16 says
//
//   macro implementation has incompatible shape:
//    required: (c: ...whitebox.Context)(): c.Expr[Int]
//   ...parameter lists have different length, found extra parameter x
import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context

object Macros {
  def implF(c: Context)(x: Int): Int = x
}

object Sugar {
  def f(): Int = macro Macros.implF
}

object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}
