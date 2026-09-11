// `updated` / `:+` / `+:` / `padTo` take `[B >: A]`: the new element type is
// at least the receiver's, and a declared lower bound joins by the plain lub
// (`AnyVal`, not the weak `Double`). scala-rs typed `Vector(1, 2).updated(0,
// "s")` at an element type one element did not have, and reading it threw.
object Main {
  def main(args: Array[String]): Unit = {
    val v2 = Vector(1, 2).updated(0, "s")
    println(v2(0) + " " + v2(1))
    val l2 = List(1, 2).updated(0, "s")
    println(l2(0) + " " + l2(1))
    val a2 = Vector(1) :+ "x"
    println(a2(1) + " " + a2(0))
    val p2 = "y" +: Vector(1)
    println(p2(0) + " " + p2(1))
    println(Vector(1, 2).updated(0, 2.5) + " " + (List(1, 2) :+ 2.5) + " " + (0.5 +: List(1, 2)) + " " + List(1).padTo(3, 1.5))
    val m: Map[String, Any] = Map("a" -> "x")
    val u = m.updated("b", 2)
    println(u("a") + " " + u("b"))
    val m2 = Map("a" -> 1).updated("c", "s")
    println(m2("a") + " " + m2("c"))
    def g[A](a: A): Vector[A] = Vector.empty :+ a
    println(g("z") ++ g(3))
  }
}
