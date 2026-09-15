import scala.reflect.runtime.universe._
import scala.reflect.runtime.{currentMirror => cm}
import scala.tools.reflect.ToolBox
object Main extends App {
  val tb = cm.mkToolBox()
  val tree = Select(This(cm.staticPackage("scala").moduleClass), TermName("Predef"))
  println(tb.eval(tb.untypecheck(tb.typecheck(tree))) == Predef)
  println(cm.staticModule("scala.Predef").fullName)
}
