import scala.language.experimental.macros
object Bad { def sum(x: Int): Int = macro typeidentitymacro.Impl.sum }
