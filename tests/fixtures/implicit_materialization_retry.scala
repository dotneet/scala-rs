package implicitretry

import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

trait Evidence[A] {
  def value: Int
}

trait LowPriorityEvidence {
  implicit def fallback[A]: Evidence[A] = new Evidence[A] {
    def value: Int = 42
  }
}

trait Supported[A]

object Supported {
  implicit def materialize[A]: Supported[A] = macro Implementation.unsupported[A]
}

object Evidence extends LowPriorityEvidence {
  implicit def specialized[A](implicit supported: Supported[A]): Evidence[A] =
    new Evidence[A] {
      def value: Int = 99
    }
}

object Query {
  def value[A]: Int = macro Implementation.value[A]
}

object Implementation {
  def unsupported[A: c.WeakTypeTag](c: Context): c.Expr[Supported[A]] =
    c.abort(c.enclosingPosition, "specialized candidate declined")

  def value[A: c.WeakTypeTag](c: Context): c.Expr[Int] = {
    import c.universe._
    val evidence = c.inferImplicitValue(weakTypeOf[Evidence[A]])
    if (evidence == EmptyTree)
      c.abort(c.enclosingPosition, "no evidence found")
    c.Expr[Int](q"$evidence.value")
  }
}
