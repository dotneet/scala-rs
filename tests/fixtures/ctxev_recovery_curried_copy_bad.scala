class P { private def copy(s: String): Int = s.length }; case class C(x: Int)(val y: Int) extends P; object Main { val c = C(1)(2).copy(3)(4) }
