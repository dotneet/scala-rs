// What the `c.prefix` bridge deliberately does not carry, named one by one.
// `docs/macros.md` §7.12. Compiled against `ex_impl.scala`'s class files.
//
// A macro is never quietly accepted: a macro def has no bytecode at all, so a
// silent pass would emit a call to a method that is not there. Real scalac
// compiles both of these; they are scala-rs's gaps, and each says so.
import scala.language.experimental.macros

class ExGap(val tag: String) {
  def label: String = macro ExImpl.tagImpl

  // Called with no receiver at all: nsc synthesises `This(ExGap)` for the
  // prefix, which is a tree the bridge does not build yet.
  def relabel: String = label
}

object ExGapMain {
  def main(args: Array[String]): Unit = {
    // A receiver the bridge cannot carry as written syntax. The expansion is
    // typechecked again at the call site, so passing a `new` across would
    // evaluate it twice; §4.3's typed-tree route is what fixes this.
    println(new ExGap("x").label)
  }
}
