// A local `case class` that reads an enclosing-method local: real scalac
// gives the synthetic companion a capture field too (`private final int
// base$1` on `Main$Q$2$`, verified with `javap` against scalac 2.13.16) and
// constructs a fresh companion instance at each `Q(...)` call site instead of
// a `MODULE$` singleton. scala-rs does not implement that shape yet -- the
// same `LazyRef` gap `check_local_objects` already refuses for a plain local
// `object` that reads outside itself. Refuse this at compile time too,
// rather than type-checking it and then throwing `NoSuchMethodError`
// building `Main$Q$1` from the companion's `apply` (the class itself gets a
// correct capturing constructor; only the companion never learns to fill it
// in).
object Main {
  def main(a: Array[String]): Unit = {
    val base = 10
    case class Q(n: Int) { def total: Int = n + base }
    println(Q(5).total)
  }
}
