// A fixed-arity alternative beside its own varargs sibling, in plain Scala.
//
// Nothing about this is Java-specific -- `java.lang.reflect.Array.newInstance`
// is only where the standard library trips over it, and `mutable.Buffer`
// declares `prepend(elem: A)` beside `prepend(elems: A*)` with no Java in
// sight. scalac 2.13.16 takes the fixed-arity alternative for a call that both
// accept, because a declared `T*` read as an argument type conforms to no
// ordinary formal: the varargs signature is not as specific as the fixed-arity
// one, while the fixed-arity one is as specific as it.
//
// Every alternative prints which one ran. Picking the varargs one where scalac
// picks the fixed one allocates a sequence and calls a different method, which
// no amount of "it compiles" would catch.
//
// Nothing here needs `Seq` or a member of a repeated parameter, so it runs in
// the private runtime as well as against the real jar. The `_*` splice and the
// Java half are in `jvarargs_java.scala`.
package jvarargs

object Main {
  def g(x: Int): String = "fixed(" + x + ")"
  def g(x: Int*): String = "varargs"

  // The same pair behind a fixed prefix, which is the shape
  // `newInstance(Class[_], Int)` / `newInstance(Class[_], Int*)` has.
  def h(tag: String, y: Int): String = "h-fixed(" + tag + "," + y + ")"
  def h(tag: String, y: Int*): String = "h-varargs(" + tag + ")"

  def main(args: Array[String]): Unit = {
    // Both alternatives accept this; the fixed-arity one wins.
    println(g(1))
    // Only the varargs one accepts these.
    println(g(1, 2))
    println(g())
    println(h("a", 1))
    println(h("a", 1, 2))
  }
}
