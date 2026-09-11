// Classes this run is compiling, asked about by a macro
// (`gbmac_decls_impl.scala`). Real scalac 2.13.16 compiles this file against
// the same implementation, and `crates/cli/tests/gbmac.rs` requires the two
// programs to print the same thing: every flag, parameter name, type and
// companion the mirror reports is the one nsc's typer reports.
//
// The shapes are the ones nsc makes more than one symbol of, or none: a case
// class's constructor `val`s (a getter and a `private[this]` field whose
// name ends in a space), a `var` (a setter too), a body `val`, a `lazy val`
// (its accessor alone), a `private[this] val` (its field alone), a plain
// class's non-`val` parameter (a field alone), a trait's `val`s (accessors
// alone, and a `$init$`), a pure trait (`<interface>`, no `$init$`), abstract
// `val`s and `var`s; the synthetic members of a case class and of its
// companion, with and without defaults; an explicit companion; a case class
// implementing a trait's abstract `val`; and a one-field case class, whose
// companion has no `tupled`.
package gbmac
package use

trait Named {
  val name: String
  val tag: String = "t"
  var count: Int = 0
  lazy val lz = 1
  def d: Int
  def e: Int = 2
  protected val pv = 3
  private val priv = 4
  def usePriv = priv
}
trait Pure {
  def a: Int
  def b(x: String): Int
}
trait OnlyDefs {
  def a: Int = 1
}

case class Foo(id: Int, name: String)
case class Bar(id: Int, var n: Option[String] = None) {
  val extra: Int = 3
  lazy val lz: String = "x"
  def m(x: Int): Int = x + extra
  private[this] val hidden = 1
  def h = hidden
}
case class One(v: Long)
case class WithObj(a: Int, b: Int)
object WithObj {
  def make: WithObj = WithObj(1, 2)
}
case class Impl(name: String, when: java.util.Date) extends Named {
  def d = 1
}
class Plain(val a: Int, b: String) {
  def g: String = b
}
abstract class AbsC(val x: Int) {
  val y: Int
  var z: Int
  def w: Int
  private var pz = 1
  protected[this] val ptv = 2
  implicit val iv: Int = 4
}
class Sub(p: Int) extends AbsC(p) {
  val y = 1
  var z = 2
  def w = 3
}

object Main {
  def main(args: Array[String]): Unit = {
    print(Decls.show[Foo])
    print(Decls.show[Bar])
    print(Decls.show[One])
    print(Decls.show[WithObj])
    print(Decls.show[Impl])
    print(Decls.show[Plain])
    print(Decls.show[Named])
    print(Decls.show[Pure])
    print(Decls.show[OnlyDefs])
    print(Decls.show[AbsC])
    print(Decls.show[Sub])
  }
}
