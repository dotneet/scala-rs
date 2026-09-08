// SLS 5.3.2: a `case class` and a `case object` get a synthesized
// `canEqual(that: Any): Boolean`. It is the one member of `scala.Equals`
// that `AnyRef` does not already supply, so without it a case class whose
// `Product` chain is visible is not a concrete class at all.
//
// `MyEquals` stands in for `scala.Equals` so that the declaration is a
// *source* declaration in this run -- the shape that `src/library`'s own
// `TupleN`, `Some`, `Left`, `Right`, `Success`, `Failure` and `None` have,
// and the shape that used to be reported "needs to be abstract".
trait MyEquals {
  def canEqual(that: Any): Boolean
}

case class Point(x: Int, y: String) extends MyEquals

case object Origin extends MyEquals

// A hand-written `canEqual` wins over the synthesized one, and the
// synthesized `equals` ends with `that.canEqual(this)` -- so two equal-valued
// `Tagged`s are unequal, while `eq` still short-circuits to true.
case class Tagged(n: Int) {
  override def canEqual(that: Any): Boolean = false
}

class Plain(val z: Int)

object Main {
  def main(args: Array[String]): Unit = {
    val p = Point(1, "a")
    val q = Point(1, "a")
    val r = Point(2, "a")
    println(p.canEqual(q))
    println(p.canEqual("x"))
    println(p.canEqual(Origin))
    println(p == q)
    println(p == r)
    // Dispatched through the declaring trait: the synthesized member really
    // implements `MyEquals`, it is not only visible on the class.
    val m: MyEquals = p
    println(m.canEqual(q))
    println(Origin.canEqual(Origin))
    println(Origin.canEqual(p))
    println(Origin == Origin)
    val t1 = Tagged(1)
    val t2 = Tagged(1)
    println(t1.canEqual(t2))
    println(t1 == t2)
    println(t2 == t1)
    println(t1 == t1)
    // The rest of the synthesized set, for the audit.
    println(p.productArity)
    println(p.productPrefix)
    println(p.productElement(0))
    println(p.productElement(1))
    println(p.productElementName(0))
    println(p.productElementName(1))
    println(p.copy(y = "b"))
    println(p.toString)
    println(Origin.productArity)
    println(Origin.productPrefix)
    println(Origin.toString)
    println(new Plain(3).z)
  }
}
