// A class in `scala.collection` that is no collection keeps its own
// declared `foreach` parameter: `mutable.HashMap.Node[K, V]` declares
// `foreach[U](f: ((K, V)) => U)`, and the element guess for collection
// receivers (the first type argument, `K`) must not replace it when the
// declared type is written in type parameters that are in scope.
package scala.collection.libappprobe {
  final class Node[K, V](val key: K, val value: V, var next: Node[K, V]) {
    def foreach[U](f: ((K, V)) => U): Unit = {
      f((key, value))
      if (next ne null) next.foreach(f)
    }
  }

  class Holder[K, V](root: Node[K, V]) {
    def foreach[U](f: ((K, V)) => U): Unit = root.foreach(f)
    def keys: List[K] = { var acc = List.empty[K]; root.foreach(kv => acc = kv._1 :: acc); acc.reverse }
  }

  object Use {
    def values[A, B](n: Node[A, B]): List[B] = { var acc = List.empty[B]; n.foreach(kv => acc = kv._2 :: acc); acc }
  }
}

package libappuse {
  import scala.collection.libappprobe._
  object Top {
    def run(): Unit = {
      val n = new Node(1, "a", new Node(2, "b", null))
      new Holder(n).foreach(kv => println(kv))
      println(new Holder(n).keys)
      println(Use.values(n))
    }
  }
}

object Main {
  def main(args: Array[String]): Unit = libappuse.Top.run()
}
