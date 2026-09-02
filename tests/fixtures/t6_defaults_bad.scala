// The other side of "a default is typed where it was written": names that are
// not in *that* scope are errors, however convenient they look at the call
// site.
//
// 1. `Pair`'s `b` defaults to the constructor parameter `a`. nsc accepts it by
//    emitting `Pair$default$2(a: Int)` on the companion object; this compiler
//    synthesizes no such getter and splices the expression instead, so there
//    is no `a` to read and it says so. What it must never do is resolve `a` to
//    the *field* -- the spliced tree would then load it off whatever `this`
//    the caller had -- nor to the caller's own local `a`.
//
// 2. `Only.tag` defaults to `Hidden.value`, and `Hidden` is imported in no
//    scope at all. Reaching it from the call site's package would be the old,
//    too-loose package walk.

package t6secret {
  object Hidden {
    val value: String = "s"
  }
}

package t6decl {
  class Pair(val a: Int, val b: Int = a)

  object Only {
    def tag(n: Int, label: String = Hidden.value): String = label + n
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val a = 99
    println(new t6decl.Pair(1).b + a)
    println(t6decl.Only.tag(2))
  }
}
