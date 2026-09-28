// The typed trees a macro receives have nsc's shape: an empty-paren Java
// method used without its parentheses is applied, inferred type arguments
// are spelled out, and a static path starts at its package -- through the
// `scala` package object for what that object aliases.
import shape.Code

object Main {
  def t(x: Int, s: String): Unit = {
    println(Code.show(x + 3))
    println(Code.show(s.length))
    println(Code.show(s.trim.length))
    println(Code.show(List(x, 3).sum))
    println(Code.show(Option(x).isEmpty))
    println(Code.show(Nil.isEmpty))
    println(Code.show(List(x, 3).map(_ + 1)))
    println(Code.rebuild(List(x, 3).sum + s.length))
    println(Code.retyped(List(x, 3).map(_ + 1).sum + s.trim.length))
  }

  def shadowed(x: Int): String = {
    // A local named `scala` does not capture the path the macro hands back.
    val scala = "shadow"
    Code.retyped(List(x).sum).toString + Code.show(List(x).sum).length + scala
  }

  def main(args: Array[String]): Unit = {
    t(5, " ab ")
    println(shadowed(7))
  }
}
