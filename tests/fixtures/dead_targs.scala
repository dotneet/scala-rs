import scala.language.implicitConversions

trait TT[T] { def name: String }
class Rep[T](val n: String)
class Node(val s: String)
class FunSym(val name: String)

/** Reached through an implicit conversion, like slick's
  * `FunctionSymbolExtensionMethods`. */
final class FunSymOps(val fs: FunSym) {
  def column[T: TT](ch: Node*): String =
    fs.name + "/" + implicitly[TT[T]].name

  /** Overloaded: the explicit type argument has to pick the generic one. */
  def typed(tpe: String, ch: Node*): String = fs.name + " raw " + tpe
  def typed[T: TT](ch: Node*): String = fs.name + " typed " + implicitly[TT[T]].name
}
object FunSymOps {
  implicit def funSymOps(fs: FunSym): FunSymOps = new FunSymOps(fs)
}

import FunSymOps._

object Lib {
  val Abs = new FunSym("abs")
  val Eq = new FunSym("==")
}

object Api {
  def mk[T](x: Int)(implicit tt: TT[T]): String = tt.name + ":" + x
  def only[T](implicit tt: TT[T]): String = tt.name
  def take[T](r: Rep[T]): String = r.n
}

/** The implicit lives on the parent and is declared in terms of *its* type
  * parameter; seen from `Numeric` it is `TT[P1]` of `Numeric`. */
trait Base[P1] {
  protected[this] def rep: Rep[P1]
  protected[this] implicit def p1Type: TT[P1]
  protected[this] def viaImplicitly: String = implicitly[TT[P1]].name
  // `take[T](r: Rep[T])` must infer `T = P1`, not widen the parameter to
  // `Rep[Any]` (which nothing of type `Rep[P1]` conforms to).
  protected[this] def viaArg: String = Api.take(rep)
}

trait Numeric[P1] extends Base[P1] {
  def abs(n: Node): String = Lib.Abs.column[P1](n)
  def report: String = viaImplicitly + "|" + viaArg
}

class IntOps(val rep0: Rep[Int]) extends Numeric[Int] {
  protected[this] def rep: Rep[Int] = rep0
  protected[this] implicit def p1Type: TT[Int] = Main.ttInt
}

/** The inherited `tpe` is a candidate here too (it reads as `TT[T]` of
  * `ConstColumn`), but the class's own context-bound evidence outranks it —
  * otherwise this is a spurious ambiguity (slick's `Rep.TypedRep`). */
abstract class TypedRep[T](val label: String) {
  implicit val tpe: TT[T] = new TT[T] { def name = label }
  def show: String = tpe.name
}
class ConstColumn[T: TT](val v: String) extends TypedRep[T]("inherited") {
  def again: ConstColumn[T] = new ConstColumn[T](v + "!")
  def evidenceName: String = implicitly[TT[T]].name
}

/** The parent's constructor parameter is declared in terms of *its* type
  * parameter, so `TT[A]` of `Wrap` has to be read as `TT[T]` of `ReWrap`
  * before the argument is checked against it. */
class Wrap[A](val tt: TT[A]) {
  def wname: String = tt.name
}
class ReWrap[T: TT](val v: String) extends Wrap[T](implicitly[TT[T]])

object Main {
  implicit val ttInt: TT[Int] = new TT[Int] { def name = "int" }
  implicit val ttBool: TT[Boolean] = new TT[Boolean] { def name = "bool" }

  def main(args: Array[String]): Unit = {
    val n = new Node("a")
    println(Lib.Abs.column[Int](n))
    println(Lib.Abs.column[Boolean](n, n))
    println(Lib.Eq.typed("t", n))
    println(Lib.Eq.typed[Boolean](n))
    println(Api.mk[Boolean](3))
    println(Api.only[Int])
    val ops = new IntOps(new Rep[Int]("r"))
    println(ops.abs(n))
    println(ops.report)
    println(new ConstColumn[Int]("c").again.v + "/" + new ConstColumn[Boolean]("d").evidenceName)
    println(new ReWrap[Int]("z").wname)
  }
}
