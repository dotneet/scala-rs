trait Repo {
  // A member that reads through `SetOps.apply` -- forcing the trait's own
  // `apply` (and, as a side effect, the companion's) to complete from the
  // jar -- before any companion `apply` call appears in the same
  // compilation unit. This is the shape the original report used. `A` is
  // fixed to `String` here, not left as the trait's own type parameter: an
  // abstract element type sends `xs(tag)` through a *different*, pre-existing
  // specificity gap (a fixed-arity parameter and a repeated one are not
  // ranked against each other when both erase from the same type variable),
  // unrelated to the pickle/prelude duplicate this fixture is about.
  def hasTag(xs: Set[String], tag: String): Boolean = xs(tag)
}
object RepoImpl extends Repo

object Main {
  def main(args: Array[String]): Unit = {
    // 1) member apply first, companion apply second -- the reported order.
    val u: Set[String] = Set("x")
    println(RepoImpl.hasTag(u, "x"))
    println(RepoImpl.hasTag(u, "y"))
    println(Set("admin"))

    // 2) the reverse order: companion apply first, member apply forced after.
    println(Set("first"))
    val v: Set[Int] = Set(1, 2, 3)
    println(v(2))
    println(v(9))

    // 3) Map: member `apply(k): V` (MapOps) vs companion `apply(kvs*): Map`.
    val m: Map[String, Int] = Map("a" -> 1, "b" -> 2)
    println(m("a"))
    println(Map("c" -> 3))

    // 4) List / Seq: element `apply(i): A` vs companion `apply(xs*): List`.
    val xs: List[Int] = List(10, 20, 30)
    println(xs(1))
    println(List(7, 8, 9))
    val ys: Seq[Int] = Seq(4, 5, 6)
    println(ys(0))
    println(Seq(1, 2))
  }
}
