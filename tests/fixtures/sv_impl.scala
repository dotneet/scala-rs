// The three things slick's `ShapedValue.mapToImpl` needs that scala-rs did
// not have (`docs/macros.md` §7.16):
//
//   1. a macro implementation whose `Context` is *refined*
//      (`blackbox.Context { type PrefixType = … }`), which is how a macro
//      says what `c.prefix` is;
//   2. `rTag.tpe.decls.collect { case s: TermSymbol => … }` -- enumerating a
//      case class's fields, which needs `MemberScope` to be an
//      `Iterable[Symbol]` and needs that scope's element type to really be
//      `Symbol`;
//   3. `..$xs` spliced *among* ordinary elements, in an argument clause, in a
//      block, and in a template body.
//
// Compiled on its own so `sv_use.scala` can expand against it, the split nsc
// requires (§1.3). Everything is in one file because the dual run compiles it
// twice, once with real scalac.
import scala.reflect.macros.blackbox

/** What the refinement pins `PrefixType` to. The macro *definition* is in
  * `sv_use.scala`, on a subclass: nsc expands a macro only against an
  * implementation compiled in an earlier run, so the two cannot share a file
  * here the way `slick.lifted.ShapedValue` does. */
class SvBox[U](val tag: String)

object SvImpl {

  /** `blackbox.Context { type PrefixType = SvBox[U] }` -- nsc's own idiom, and
    * exactly how slick declares `mapToImpl`. */
  def describeImpl[R, U](
      c: blackbox.Context { type PrefixType = SvBox[U] }
  )(implicit rTag: c.WeakTypeTag[R]): c.Tree = {
    import c.universe._

    // The line slick's `mapToImpl` opens with. `decls` is a `MemberScope`,
    // whose only path to `collect` is `ScopeApi extends Iterable[Symbol]` --
    // two pickled parents above the stub the class file describes.
    val fields =
      rTag.tpe.decls
        .collect {
          case s: TermSymbol if s.isVal && s.isCaseAccessor =>
            (TermName(s.name.toString.trim), s.typeSignature)
        }
        .toIndexedSeq

    // The spliced lists. `names` comes from the field walk, and is empty for a
    // type with no case accessors -- the `Nil` end of the concatenation.
    // `labels` is fixed, so the *order* of a mixed splice is checked even
    // where the walk finds nothing.
    val names = fields.map { case (n, t) => q"${n.toString + ":" + t.toString}" }
    val labels = List("a", "b", "c").map(l => q"$l")
    // Statements of a block, spliced among ordinary ones.
    val appends = List("p", "q").map(l => q"sb.append($l).append(';')")
    // Members of a *template body*, spliced among ordinary ones -- the shape
    // slick builds its `SimpleFastPathResultConverter` with. An anonymous
    // class is not something scala-rs can rebuild out of an expansion yet, so
    // this one is not returned: its printed form goes into the expansion
    // instead, which is a stricter check anyway. Real scalac reifies the same
    // template with the same reflect API, so the two builds print the same
    // string only if every splice landed in the same place.
    val members = List("u", "v").map(l => q"def ${TermName("f_" + l)}: String = $l")
    val tmpl = q"""new _root_.scala.AnyRef {
      ..$members
      override def toString: String = "obj"
    }"""
    val shown = tmpl.toString.replaceAll("\\s+", " ")

    q"""
      val sb = new _root_.scala.collection.mutable.StringBuilder
      sb.append(${c.prefix}.tag).append('|')
      ..$appends
      sb.append("end")
      _root_.scala.collection.immutable.List(
        "head", ..$names, "mid", ..$labels, "tail", $shown, sb.toString
      ).mkString(",")
    """
  }

  /** The same field walk with no quasiquote at all, so a failure in the walk
    * is told apart from a failure in the reifier: the names and types come
    * back as one constant. */
  def fieldsOf[R](c: blackbox.Context)(implicit rTag: c.WeakTypeTag[R]): c.Expr[String] = {
    import c.universe._
    val fs = rTag.tpe.decls.collect {
      case s: TermSymbol if s.isVal && s.isCaseAccessor =>
        s.name.toString.trim + ":" + s.typeSignature
    }
    c.Expr[String](Literal(Constant("[" + fs.toIndexedSeq.mkString(",") + "]")))
  }
}
