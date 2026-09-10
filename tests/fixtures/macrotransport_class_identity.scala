import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context
object Factory {
 def make: AnyRef = macro impl
 def impl(c: Context): c.Tree = { import c.universe._; c.untypecheck(c.typecheck(q"{ class C; new C }")) }
}
