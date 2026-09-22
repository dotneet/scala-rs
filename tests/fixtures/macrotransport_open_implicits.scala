import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context

final case class EvidenceInfo[A](depth: Int, expected: String)
object EvidenceInfo {
  implicit def materialize[A]: EvidenceInfo[A] = macro implementation[A]
  def implementation[A: c.WeakTypeTag](c: Context): c.Expr[EvidenceInfo[A]] = {
    import c.universe._
    val pending = c.openImplicits
    val head = pending.head
    val depth = pending.length
    val expected = head.pt.toString
    val annotationTree = c.typecheck(q"""new _root_.scala.annotation.implicitNotFound("old")""")
    val updated = new Transformer {
      override def transform(tree: Tree): Tree = tree match {
        case Literal(Constant("old")) => Literal(Constant("new"))
        case other => super.transform(other)
      }
    }.transform(annotationTree)
    Annotation(updated)
    c.Expr[EvidenceInfo[A]](q"new EvidenceInfo[${weakTypeOf[A]}]($depth, $expected)")
  }
}
