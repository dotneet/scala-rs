// Generality check for the third case: a user-written implicit def unrelated to
// Ordered gets eta-expanded and passed to a function-typed implicit parameter.
// Polymorphic implicit defs and implicit defs with implicit arguments of their
// own count too. Works under the private runtime as well.
import scala.language.implicitConversions

class Tagged(val s: String) { override def toString: String = "<" + s + ">" }
trait Show[A] { def show(a: A): String }
class Wrap[B](val get: B)

object Main {
  implicit def intTagged(n: Int): Tagged = new Tagged("i" + n)
  implicit def boxAny[A](a: A)(implicit sh: Show[A]): Tagged = new Tagged(sh.show(a))
  implicit val showString: Show[String] = new Show[String] {
    def show(a: String): String = "s" + a
  }

  // A function-typed implicit parameter. The only candidate is an implicit def.
  def render[A](a: A)(implicit view: A => Tagged): String = view(a).toString
  // A view bound falls onto the same path.
  def render2[A <% Tagged](a: A): String = a.toString
  // Nested: pass our own implicit parameter on to the inner call.
  def renderPair[A](a: A, b: A)(implicit view: A => Tagged): String =
    render(a) + "|" + render(b)

  // B appears nowhere in the call (nsc's undetermined type parameters). The witness
  // is a *conversion*, not a value, so B can only be solved from its result type.
  implicit def intWrap(n: Int): Wrap[String] = new Wrap("w" + n)
  def unwrap[A, B](a: A)(implicit view: A => Wrap[B]): B = view(a).get

  def main(args: Array[String]): Unit = {
    println(render(7))
    println(render("hi"))
    println(render2(7))
    println(render2("hi"))
    // The conversion also applies while still polymorphic.
    val t: Tagged = "zz"
    println(t)
    // The view is found through a nested implicit parameter too.
    println(renderPair(1, 2))
    println(renderPair("a", "b"))
    // Solve an undetermined type parameter from the view's result type.
    val u = unwrap(9)
    println(u.length.toString + " " + u)
  }
}
