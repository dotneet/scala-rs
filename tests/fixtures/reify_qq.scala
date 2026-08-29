// Quasiquotes, reified.
//
// `q"..."` is a compiler-internal macro in nsc: scala-reflect.jar holds no
// implementation, so scala-rs desugars it itself into the universe calls that
// build the reflect tree at run time (`docs/macros.md` §6.2, §7.3 B). Every
// line below is checked against real scalac 2.13.16, which prints the same
// thing -- the trees are the same trees.
//
// Needs scala-reflect.jar on the classpath; `import <universe>._` is what puts
// `q` in scope in the first place.
import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    // Literals and plain names.
    println(q"1")
    println(q"greet")
    println(q"true")
    println(q""" "hi" """)
    // Selections, however deep.
    println(q"a.b.c")
    // Applications, including curried ones.
    println(q"f(1)")
    println(q"a.b(1)(2)")
    // A `$` hole splices a tree straight in.
    val inner = q"x"
    println(q"g($inner)")
    println(q"h($inner, 2)")
    println(q"$inner.size")
    // A `..$` hole splices a whole argument list.
    val xs = List(q"p", q"q")
    println(q"k(..$xs)")
    println(q"k()")
  }
}
