// Auto-tupling: more arguments than the one parameter, which is a tuple,
// followed by an implicit clause (slick's `q.update("Reopen", "reopen")`).
// nsc tries the tupled application before typing any argument against the
// tuple formal; scala-rs typed `"Reopen"` against `(String, String)` first,
// where a view in scope could turn it into something else entirely.
import scala.language.implicitConversions
class S
class Q[U] {
  def update(v: U)(implicit s: S): Int = { println("update " + v); 1 }
}
class QQ[+E, U, C[_]](val u: U)
trait Api {
  class UpdInv[U](param: Any) {
    def update(value: U)(implicit s: S): Int = { println("inv " + value); 2 }
  }
  implicit def queryToUpdateInvoker[U, C[_]](q: QQ[_, U, C]): UpdInv[U] = new UpdInv[U](q)
}
object slickish extends Api {
  // slick's `anyToShapedValue` shape: a view out of *any* value into a class
  // of two type parameters. It must never answer a tuple.
  trait Ev[T, U]
  final case class SV[T, U](value: T, ev: Ev[T, U])
  implicit def evSame[T]: Ev[T, T] = new Ev[T, T] {}
  implicit def toSV[T, U](v: T)(implicit ev: Ev[T, U]): SV[T, U] = SV(v, ev)
}
import slickish._
object Main {
  def upd(v: (String, String))(implicit s: S): Int = { println("upd " + v); 3 }
  def g(p: (Set[Int], Int)): Int = p._1.size + p._2
  def h(p: (List[Int], String))(implicit s: S): String = p._1.mkString(",") + p._2
  def main(args: Array[String]): Unit = {
    implicit val s: S = new S
    println(upd("a", "b"))
    val q = new Q[(String, String)]
    println(q.update("Reopen", "reopen"))
    val x = "c"
    println(q.update(x, x))
    val qq = new QQ[Int, (String, String), List](("a", "b"))
    println(qq.update("Close", "close"))
    println(g(Set(), 1))
    println(h(Nil, "x"))
    println(h(List(1, 2), "y"))
  }
}
