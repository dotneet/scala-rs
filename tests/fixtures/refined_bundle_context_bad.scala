import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context
class InvalidBundle(val c: Context { type Foo <: Int }) {
  def answer: c.Tree = { import c.universe._; q"42" }
}
object InvalidBundleUse { def answer: Int = macro InvalidBundle.answer }
