// `scala.reflect.api.Printers.BooleanFlag`: a case class nested in the trait
// `Printers`, whose companion -- an inner object of that same trait, reached
// as `universe.BooleanFlag` -- declares the only `Boolean => BooleanFlag`
// conversion there is.
//
// Two mechanisms meet here. The companion has to be in the implicit scope of
// `BooleanFlag` even though the class reached the symbol table flattened
// (`Printers$BooleanFlag` in package `scala.reflect.api`) and the companion
// nested (`BooleanFlag$` owned by `Printers`), so the two share no name; and
// the conversion call needs `universe.BooleanFlag` as its receiver, because an
// object nested in a class has no `MODULE$` field.
//
// `showRaw` is overloaded four ways, so the named argument also exercises the
// second applicability pass: the view that makes `true` fit lives on the
// companion of the *parameter* type, not of `Boolean`.
import scala.reflect.runtime.universe._

object Main {
  // Symbol ids are run-dependent; the corpus's own `showraw_*` tests stabilise
  // them the same way.
  def stable(s: String): String = """#\d+""".r.replaceAllIn(s, "#<id>")

  def main(args: Array[String]): Unit = {
    val flag: BooleanFlag = true
    println(flag)
    println(BooleanFlag(None))

    val t = typeOf[Int]
    println(stable(showRaw(t)))
    println(stable(showRaw(t, printIds = true)))
    println(stable(showRaw(t, printKinds = true)))
    println(stable(showRaw(t, printIds = true, printKinds = true)))
    println(stable(s"${showRaw(t, printKinds = true)}"))

    val sym = t.typeSymbol
    println(stable(showRaw(sym, printKinds = true)))
  }
}
