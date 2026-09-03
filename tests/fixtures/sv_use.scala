// Call sites for `sv_impl.scala`, compiled against its class files the way
// nsc requires -- the implementation has to come from an earlier run.
//
// Real scalac 2.13.16 compiles the same two files against each other, and the
// two programs must print the same thing. A field walk that lost the element
// type, or a `..$` splice concatenated in the wrong order, still compiles;
// only the output tells.
//
// The types the field walk enumerates are the *library's*, not ones declared
// here. A macro reads a `WeakTypeTag`'s members through a real runtime
// mirror, which reads the class file's `ScalaSignature`, and scala-rs's own
// pickle does not record case accessors -- a case class this compiler
// produced comes back with no fields at all, so the two builds would differ
// for a reason that has nothing to do with what is under test. See README,
// "Remaining". `Deadline` is a case class with one accessor; `BigDecimal` has
// none, which is the empty end of the splice concatenation.
import scala.language.experimental.macros

/** The macro's prefix. `describe` reads `c.prefix` and puts its `tag` in the
  * expansion, so a `PrefixType` that did not survive the refinement would
  * show up in the output, not merely in the types. */
class SvTagged[U](t: String) extends SvBox[U](t) {
  def describe[R]: String = macro SvImpl.describeImpl[R, U]
}

object SvUse {
  def fieldsOf[R]: String = macro SvImpl.fieldsOf[R]
}

object Main {
  def main(args: Array[String]): Unit = {
    val box = new SvTagged[Int]("BOX")
    println(box.describe[scala.concurrent.duration.Deadline])
    println(box.describe[scala.math.BigDecimal])
    println(SvUse.fieldsOf[scala.concurrent.duration.Deadline])
    println(SvUse.fieldsOf[scala.math.BigDecimal])
  }
}
