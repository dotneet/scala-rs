// A default argument's right-hand side means what it meant *where it was
// written*, not what its names happen to mean at the call site.
//
// `t6lib` is the only place that imports `t6cfg`; `Main` sits in the root
// package and cannot see `Names` or `Tag` unqualified. Every default below is
// therefore only resolvable in the file's own scope, and each one is reached
// through a different route: a positional constructor call, a named
// constructor call, an ordinary method, and an implicit parameter whose
// search comes up empty.

package t6cfg {
  object Names {
    val greeting: String = "hello"
    val level: Int = 3
  }
  trait Tag[T] {
    def name: String
  }
  object Tags {
    val forInt: Tag[Int] = new Tag[Int] {
      def name: String = "int"
    }
  }
}

package t6lib {
  import t6cfg.{Names, Tag}

  class Box(val n: Int, val label: String = Names.greeting, val depth: Int = Names.level) {
    override def toString: String = "Box(" + n + "," + label + "," + depth + ")"
  }

  object Mk {
    def make(n: Int, label: String = Names.greeting): String = "make(" + n + "," + label + ")"

    // An implicit parameter that carries a default: when the search finds
    // nothing, nsc uses the default rather than reporting a missing implicit.
    def pick[T](implicit t: Tag[T] = null): String =
      if (t == null) Names.greeting else t.name
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new t6lib.Box(1))
    println(new t6lib.Box(2, depth = 9))
    println(new t6lib.Box(3, label = "x"))
    println(t6lib.Mk.make(4))
    println(t6lib.Mk.make(5, label = "y"))
    implicit val ti: t6cfg.Tag[Int] = t6cfg.Tags.forInt
    println(t6lib.Mk.pick[Int])
    println(t6lib.Mk.pick[String])
  }
}
