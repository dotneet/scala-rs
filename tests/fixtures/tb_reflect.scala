// Runtime reflection reached through `currentMirror`, and the toolbox reached
// through the implicit conversion `scala.tools.reflect.ToolBox`.
//
// Every line here needs something this slice added:
//   * `currentMirror` is one of nsc's fast-track macros -- it has no bytecode
//     to invoke, and scala-rs expands it itself
//     (crates/typer/src/fasttrack_mirror.rs);
//   * `classSymbol` / `moduleSymbol` take a `RuntimeClass`, which is abstract
//     where they are declared and `java.lang.Class[_]` as a `JavaUniverse`'s
//     mirror sees it;
//   * `def nothing = ???` reads back as `Nothing` only if the emitted pickle
//     says so;
//   * `mkToolBox` is a member of what the *package object*'s implicit
//     conversion returns, which the import has to bring into scope alongside
//     the trait of the same name;
//   * `tb.typecheck(t)` fills in five defaulted parameters.
//
// Compiles and runs identically under real scalac 2.13.16.
import scala.reflect.runtime.universe._
import scala.reflect.runtime.{currentMirror => cm}
import scala.tools.reflect.ToolBox

class TbA {
  def nothing = ???
  def name: String = "a"
}

object TbCompanion {
  def value = 42
}

object Main {
  def main(args: Array[String]): Unit = {
    val c = cm.classSymbol(classOf[TbA])
    println(c)
    println(c.fullName)
    println(c.info)

    val m = cm.moduleSymbol(Class.forName("TbCompanion$"))
    println(m.fullName)

    // The mirror obtained the long way round is the same one.
    val explicit = runtimeMirror(classOf[TbA].getClassLoader)
    println(explicit.classSymbol(classOf[TbA]).fullName)

    val tb = cm.mkToolBox()
    println(tb.eval(Literal(Constant(3))))
    // `typecheck` has five defaulted parameters; the getter for each is a
    // `$default$n` the pickle and the class file both declare.
    println(tb.typecheck(Literal(Constant("s"))).tpe.toString)
  }
}
