import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object Marker {
  def value(i: Int): Int = macro MarkerImpl.value
}

object MarkerImpl {
  def value(c: blackbox.Context)(i: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](q"$i + 1")
  }
}
