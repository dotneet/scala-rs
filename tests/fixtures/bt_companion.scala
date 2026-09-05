// A companion object read from a class file must not be shadowed by its class
// in term position.
//
// `docs/gitbucket.md`'s "what would remove the most next", entry 6. A class
// file's companion is a *second* class file, installed only when something
// asks for that name. `import btc._` over a package asks for no name in
// particular: the eager half of a wildcard import walks the package's member
// list and enters what is already there, which for a `-cp` package is the
// classes alone. Once `Holder` was in the current scope, name resolution
// stopped at the first line of `expose_unqualified` -- the name resolves --
// and the companion was never read.
//
// `Holder[Int](3)` then bound the *class*, whose type (a class file's, so
// with no type arguments of its own) is not applied to anything, and the
// `Module[T]` → `Module.apply[T]` redirect never ran because it needs a
// module symbol. The symptom is downstream: `get` came back as the bare `T`.
// slick writes `val TableQuery = lifted.TableQuery` inside `profile.api`, and
// a *term* is entered by the same walk, which is why gitbucket never hit this.
//
// Compiled against `bt_companion_lib.scala`'s class files. Real scalac
// 2.13.16 compiles both halves and prints the same output.
import btc._

object Main {
  def main(args: Array[String]): Unit = {
    // Term position: the companion's `apply`, not the class.
    val h = Holder[Int](3)
    println(h.get + 1)
    println(Holder("s").get.length)
    // A factory with no value parameters -- the `TableQuery[E]` shape.
    val e: Empty[Int] = Empty[Int]
    println(e.name)
    // Type position still names the class.
    val t: Holder[String] = Holder[String]("x")
    println(t)
    // An ordinary selection on the companion.
    println(Holder.tag)
    // And `new` still names the class.
    println(new Holder[Long](7L).get)
  }
}
