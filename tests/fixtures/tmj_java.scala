// `scala/collection/concurrent/TrieMap.scala`'s configuration, reduced: a
// Scala class extending a *generic Java* class that only exists as a class
// file, which
//
//   * instantiates itself with its own type parameters written out, in a
//     method with no declared result type (`CNode.renewed` and friends), and
//   * reads the Java superclass's package-private statics, both qualified
//     (`INodeBase.RESTART`) and through `import INodeBase._`, which is how
//     `INode` gets at `NO_SUCH_ELEMENT_SENTINEL`.
//
// Both were errors: the first because the written type-argument list was
// discarded as if it were a placeholder, the second because the class-file
// reader kept only `public` and `protected` members and dropped default
// access entirely.
package tmjava

final class SNode[K, V](k: K, v: V, val tag: String) extends JBase[K, V](k, v) {
  // The wildcard import of a Java class's static scope. Note that Scala does
  // *not* inherit Java statics into a subclass's scope -- real scalac rejects
  // a bare `SENTINEL` without this import -- so the import is load-bearing,
  // exactly as it is at the top of `class INode` in TrieMap.scala.
  import JBase._

  def retagged(t: String) = new SNode[K, V](key, value, t)

  def describe(): String = tag + ":" + value

  def sentinel: String = SENTINEL

  def restart: String = JBase.RESTART
}

object Main {
  def tagOf[K, V](n: SNode[K, V]): String = n.tag

  def main(args: Array[String]): Unit = {
    val n = new SNode[Int, String](7, "seven", "a")

    val m: SNode[Int, String] = n.retagged("b")
    println(m.key)
    println(m.value)
    println(m.describe())
    println(tagOf(n.retagged("c")))

    println(n.sentinel)
    println(n.restart)
    println(JBase.PUBLIC_TAG)
  }
}
