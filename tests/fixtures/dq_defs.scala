// Quasiquoted *definitions*: `class`, `case class`, `trait`, `object`, `def`,
// and a `val` carrying modifiers. `docs/macros.md` §7.8.
//
// Every line is compared with real scalac 2.13.16, which prints
// `expected/dq_defs.txt`: `showRaw` means the comparison is of the *trees*,
// not of anything that merely typechecks. The shapes were read off nsc with
// `-Ymacro-debug-lite`, which prints what its own quasiquote macro expands to
// -- including the `Modifiers` flag bits, which are
// `scala.reflect.internal.Flags`' numbering and not the parser's.
//
// Needs scala-reflect.jar on the classpath; `import <universe>._` is what puts
// `q` in scope in the first place.
import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    val x: Tree = q"x"
    val t: Tree = tq"Int"
    val tname: TypeName = TypeName("C")
    val n: TermName = TermName("f")
    val params: List[Tree] = List(q"val a: Int")
    val parents: List[Tree] = List(tq"D")
    val body: List[Tree] = List(q"def g = 1")

    // --- classes ---------------------------------------------------------
    println(showRaw(q"class C"))
    println(showRaw(q"class C()"))
    println(showRaw(q"class C()()"))
    println(showRaw(q"class C {}"))
    println(showRaw(q"class C(x: Int)"))
    println(showRaw(q"class C(x: Int, y: String)"))
    println(showRaw(q"class C(val x: Int)"))
    println(showRaw(q"class C(var x: Int)"))
    println(showRaw(q"class C(private val x: Int)"))
    println(showRaw(q"class C(protected val x: Int)"))
    println(showRaw(q"class C(implicit x: Int)"))
    println(showRaw(q"class C(x: Int)(implicit y: Int)"))
    println(showRaw(q"class C private (x: Int)"))
    println(showRaw(q"class C[T]"))
    println(showRaw(q"class C[+T <: AnyRef]"))
    println(showRaw(q"class C[T](x: T)"))
    println(showRaw(q"class C extends D"))
    println(showRaw(q"class C extends D with E"))
    println(showRaw(q"class C extends D(1) with E"))
    println(showRaw(q"class C(x: Int) extends D(x)"))
    println(showRaw(q"class C { def g = 1 }"))
    println(showRaw(q"class C { def a = 1; def b = 2 }"))
    println(showRaw(q"class C { val y: Int = 1 }"))
    println(showRaw(q"class C { private val y = 1 }"))
    println(showRaw(q"final class C"))
    println(showRaw(q"abstract class C"))
    println(showRaw(q"sealed class C"))
    println(showRaw(q"private class C"))
    println(showRaw(q"implicit class C(x: Int)"))
    println(showRaw(q"abstract class C { def g: Int }"))

    // --- case classes: nsc supplies `Product with Serializable` -----------
    println(showRaw(q"case class C(x: Int)"))
    println(showRaw(q"case class C()"))
    println(showRaw(q"case class C(val x: Int)"))
    println(showRaw(q"case class C(private val x: Int)"))
    println(showRaw(q"case class C(x: Int)(y: Int)"))
    println(showRaw(q"case class C(x: Int, y: String) extends D"))
    println(showRaw(q"case class C(x: Int) extends AnyRef"))
    println(showRaw(q"final case class C(x: Int)"))

    // --- traits and objects ----------------------------------------------
    println(showRaw(q"trait T"))
    println(showRaw(q"sealed trait T"))
    println(showRaw(q"trait T { def g: Int }"))
    println(showRaw(q"object O"))
    println(showRaw(q"object O {}"))
    println(showRaw(q"object O { def g = 1 }"))
    println(showRaw(q"object O extends P"))
    println(showRaw(q"object O extends P with Q"))
    println(showRaw(q"case object O"))

    // --- defs -------------------------------------------------------------
    println(showRaw(q"def f = 1"))
    println(showRaw(q"def f = { 1 }"))
    println(showRaw(q"def f: Int = 1"))
    println(showRaw(q"def f: Int"))
    println(showRaw(q"def f(x: Int): Int = x"))
    println(showRaw(q"def f(x: Int): Int"))
    println(showRaw(q"def f[T](x: T)(y: T): T = x"))
    println(showRaw(q"def f(x: Int = 1) = x"))
    println(showRaw(q"def f(implicit x: Int) = x"))
    println(showRaw(q"override def f = 1"))
    println(showRaw(q"private def f = 1"))
    println(showRaw(q"protected def f = 1"))
    println(showRaw(q"implicit def f = 1"))
    println(showRaw(q"final def f = 1"))

    // --- modified vals and vars -------------------------------------------
    println(showRaw(q"lazy val a = 1"))
    println(showRaw(q"implicit val x: Int = 1"))
    println(showRaw(q"private val x = 1"))
    println(showRaw(q"private[this] val x = 1"))
    println(showRaw(q"protected val x = 1"))
    println(showRaw(q"override val x = 1"))
    println(showRaw(q"final val x = 1"))
    println(showRaw(q"val a: Int"))
    println(showRaw(q"var x = 1"))
    println(showRaw(q"var x: Int = 1"))

    // --- definitions inside a block --------------------------------------
    println(showRaw(q"""{ case class X(a: Int); new X(1) }"""))
    println(showRaw(q"{ def g = 1; g }"))
    println(showRaw(q"{ lazy val a = 1; a }"))
    println(showRaw(q"{ class C; new C }"))
    println(showRaw(q"{ object O; O }"))
    println(showRaw(q"{ trait T; 1 }"))

    // --- an anonymous class's body ----------------------------------------
    println(showRaw(q"new C { def g = 1 }"))
    println(showRaw(q"new C(1) { override def read(r: Int): Int = r }"))
    println(showRaw(q"new C(1) { ..$body }"))
    println(showRaw(q"new { def g = 1 }"))
    println(showRaw(q"new D(1) with E"))

    // --- holes -------------------------------------------------------------
    println(showRaw(q"class $tname"))
    println(showRaw(q"class C(..$params)"))
    println(showRaw(q"class C extends ..$parents"))
    println(showRaw(q"class C { ..$body }"))
    println(showRaw(q"class $tname(..$params) extends ..$parents { ..$body }"))
    println(showRaw(q"class C extends D { ..$body }"))
    println(showRaw(q"object O { ..$body }"))
    println(showRaw(q"def $n = 1"))
    println(showRaw(q"def f(..$params) = 1"))
    println(showRaw(q"def f(..$params): Int = 1"))
    println(showRaw(q"val v: $t = $x"))
    // A definition's body written in braces around a splice: the parser folds
    // `{ e }` down to `e`, so the braces survive only in the source text.
    println(showRaw(q"def f: Unit = {..$body}"))
    println(showRaw(q"def f = { ..$body }"))
    println(showRaw(q"val v = { ..$body }"))
    println(showRaw(q"class C { def g: Unit = {..$body} }"))

    // --- `super`, which is only ever written inside a definition ----------
    println(showRaw(q"super.foo"))
    println(showRaw(q"C.super.foo"))
    println(showRaw(q"super[D].foo"))
    println(showRaw(q"new C { override def g = super.g }"))
  }
}
