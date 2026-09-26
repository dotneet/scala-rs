package bynameanswer

import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context

// Overloaded on its first parameter with a by-name second one, as circe's
// `DecodingFailure.apply(String | Reason, => List[CursorOp])` is.
object Failure {
  def apply(message: String, history: => List[Int]): String = message + history.mkString("(", ",", ")")
  def apply(code: Int, history: => List[Int]): String = code.toString + history.mkString("(", ",", ")")
}
trait Deferred[A] { def value: String }
object Deferred {
  def make[A](value: String): Deferred[A] = {
    val v = value
    new Deferred[A] { def value: String = v }
  }
  // The typed expansion holds the by-name argument itself; the thunk that
  // delays it only appears when the call is lowered.
  implicit def materialize[A]: Deferred[A] = macro materializeImpl[A]
  def materializeImpl[A: c.WeakTypeTag](c: Context): c.Tree = {
    import c.universe._
    val a = weakTypeOf[A]
    q"_root_.bynameanswer.Deferred.make[$a](_root_.bynameanswer.Failure(${a.typeSymbol.name.toString}, _root_.scala.List(1, 2)))"
  }
}
// Hands back the evidence it found untypechecked, as shapeless's `Lazy`
// does with every instance it collects.
object Untyped {
  def apply[A]: A = macro implementation[A]
  def implementation[A: c.WeakTypeTag](c: Context): c.Tree =
    c.untypecheck(c.inferImplicitValue(c.weakTypeOf[A], silent = false))
}
