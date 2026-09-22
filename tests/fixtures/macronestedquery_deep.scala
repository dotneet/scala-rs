import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context

final case class DeepBox[A](value: A)

trait DeepEvidence[A] {
  def depth: Int
}

object DeepEvidence {
  implicit def derive[A]: DeepEvidence[A] = macro implementation[A]

  def implementation[A: c.WeakTypeTag](c: Context): c.Tree = {
    import c.universe._
    val input = weakTypeOf[A]
    if (input =:= typeOf[Int]) {
      q"new DeepEvidence[$input] { def depth: Int = 0 }"
    } else {
      val inner = input.typeArgs.head
      val evidence = c.inferImplicitValue(
        appliedType(typeOf[DeepEvidence[Any]].typeConstructor, inner)
      )
      q"new DeepEvidence[$input] { def depth: Int = $evidence.depth + 1 }"
    }
  }
}

object DeepQuery {
  def value[A]: Int = macro implementation[A]

  def implementation[A: c.WeakTypeTag](c: Context): c.Expr[Int] = {
    import c.universe._
    val evidence = c.inferImplicitValue(
      appliedType(typeOf[DeepEvidence[Any]].typeConstructor, weakTypeOf[A])
    )
    c.Expr[Int](q"$evidence.depth")
  }
}
