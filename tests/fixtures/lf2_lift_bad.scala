// What `Liftable` still refuses, and `reify { … }`, both named.
//
// `docs/macros.md` §7.8. scala-rs knows the *standard* `Liftable` instances
// and builds the tree each of them builds; it does not search for an implicit,
// so a hole of any other type is reported with its type rather than turned
// into some other tree. `reify { … }` is a compiler-internal macro like the
// quasiquotes, with no implementation in scala-reflect.jar: saying `value
// reify is not a member of JavaUniverse` was untrue, the same way `value q is
// not a member of StringContext` was.
import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    val f: java.io.File = new java.io.File("x")
    val xs: List[Int] = List(1, 2)
    val os: List[java.io.File] = List(f)

    // No standard `Liftable[File]`; nsc would look for a user-written one.
    println(showRaw(q"g($f)"))
    // A rank-0 collection is `liftList` in nsc, which builds a
    // `List(...)` *call*, not the splice `..$xs` builds. scala-rs does not
    // build that instance, and refuses rather than approximate it.
    println(showRaw(q"g($xs)"))
    // `..$` over elements with no instance is reported by the element type.
    println(showRaw(q"g(..$os)"))
    // A `Symbol` *is* lifted on its own (`mkRefTree`), but nsc has no
    // `Liftable[Symbol]`, so under `..$` it refuses -- and so do we.
    val syms = List(definitions.ListModule, definitions.ListModule)
    println(showRaw(q"g(..$syms)"))
    // A compiler-internal macro, unqualified and qualified.
    println(reify(1).toString)
    println(scala.reflect.runtime.universe.reify(2).toString)
  }
}
