package shapeprobe

import scala.language.experimental.macros
import scala.reflect.macros.blackbox

trait NestedShape[A]

object NestedShape {
  implicit val intShape: NestedShape[Int] = new NestedShape[Int] {}
  implicit val longShape: NestedShape[Long] = new NestedShape[Long] {}
  implicit val stringShape: NestedShape[String] = new NestedShape[String] {}
  implicit def derived[A]: NestedShape[A] = macro NestedShapeImpl.derived[A]
}

object NestedShapeImpl {
  def derived[A: c.WeakTypeTag](c: blackbox.Context): c.Expr[NestedShape[A]] = {
    import c.universe._
    val target = weakTypeOf[A].dealias
    val parts = target.typeArgs
    if (parts.isEmpty) c.abort(c.enclosingPosition, "unsupported shape")
    parts.foreach { part =>
      val required = appliedType(typeOf[NestedShape[_]].typeConstructor, part)
      if (c.inferImplicitValue(required) == EmptyTree)
        c.abort(c.enclosingPosition, "missing nested shape")
    }
    c.Expr[NestedShape[A]](q"new NestedShape[$target] {}")
  }

  def spliced[A: c.WeakTypeTag](c: blackbox.Context): c.Expr[SplicedShape[A]] = {
    import c.universe._
    val target = weakTypeOf[A].dealias
    val parts = target.typeArgs
    if (parts.isEmpty) c.abort(c.enclosingPosition, "unsupported shape")
    val evidence = parts.map { part =>
      val required = appliedType(typeOf[SplicedShape[_]].typeConstructor, part)
      val found = c.inferImplicitValue(required)
      if (found == EmptyTree) c.abort(c.enclosingPosition, "missing nested shape")
      found
    }
    c.Expr[SplicedShape[A]](q"{ ..$evidence; new SplicedShape[$target] {} }")
  }
}

trait SplicedShape[A]

object SplicedShape {
  implicit val intShape: SplicedShape[Int] = new SplicedShape[Int] {}
  implicit val stringShape: SplicedShape[String] = new SplicedShape[String] {}
  implicit def derived[A]: SplicedShape[A] = macro NestedShapeImpl.spliced[A]
}
