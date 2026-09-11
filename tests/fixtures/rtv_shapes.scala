// Assorted miscompiled shapes (fixture prefix `rtv_`).
case class Priv(private val x: Int, y: Option[Int])
object Whatever { override def equals(x: Any) = true }

abstract class FruitFactory { def getFruit: AnyRef }
class AppleFactory extends FruitFactory {
  @Deprecated def getFruit: String = "apple"
}

class Gen[T: scala.reflect.ClassTag] { val grid = Array.ofDim[T](2, 3) }

object X {
  class Y
  def y = new Y { class Z; def z = classOf[Z] }
}

object Main {
  // A non-public case field is read through `x$access$0` outside the class
  // (`run/t5407`, `run/t8944`).
  def priv(p: Priv) = p match {
    case Priv(x, Some(y)) => x + y
    case Priv(x, _) => x
  }
  // A literal wider than the scrutinee compares in the wider type
  // (`run/t1423`).
  def lit(x: Int) = x match {
    case 0xFFFFFFFF00000001L => "overflow"
    case 1L => "one"
    case 3.0 => "three"
    case _ => "other"
  }
  // `n @ Whatever` binds at the scrutinee's type (`run/t1503`).
  def whatever(x: Any) = (x: @unchecked) match { case n @ Whatever => n }

  def nested(): Int =
    try {
      try return 10
      finally {
        try { () } catch { case _: Throwable => () }
        println("in finally 1")
      }
    } finally println("in finally 2")

  def main(args: Array[String]): Unit = {
    println(priv(Priv(1, Some(2))) + priv(Priv(5, None)))
    println(List(1, 2, 3).map(lit))
    println(whatever("w"))
    // A `try` inside a `finally` of a body that only exits by `return`
    // (`run/finally`).
    println(nested())
    // `Array[Unit]` elements are `BoxedUnit` (`run/t5680`).
    val units = Array[Unit]((), ())
    units(1) = ()
    println(units(1))
    // Arrays read back through erased results (`run/groupby`,
    // `run/t0677-new`, `run/t7015`).
    val groups = Array.range(0, 6).groupBy(_ % 2)
    println(groups(0).length)
    val g = new Gen[String]
    g.grid(1)(2) = "hello"
    println(g.grid(1)(2))
    def f: Null = null
    val nulls = Array(f, f)
    println(nulls(0))
    // A captured `var` of type `Unit` (`run/t6863`).
    var y = 1
    var u = y = 42
    println({ () => u }.apply())
    // A bridge carries the annotations of its target (`run/t10527`).
    val bridge = classOf[AppleFactory].getDeclaredMethods.filter(_.isBridge)(0)
    println(bridge.getAnnotations()(0).annotationType.getName)
    // A class declared inside an anonymous class is emitted (`run/t8445`).
    println(X.y.z.getSimpleName)
  }
}
