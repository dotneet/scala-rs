// Run-time conversions around erased values (fixture prefix `rtv_`).
//
// * An erased value unboxes through `BoxesRunTime.unboxToX`, which answers
//   the zero of the type for `null` (`run/function-null-unbox`,
//   `run/t7584b`, `run/t7899`).
// * `null.asInstanceOf[V]` for a value class is `V` over the underlying
//   type's zero (`run/t5866`).
// * A result type parameter instantiated at `Nothing` still returns:
//   `def s: String = get("s")` casts the erased result, it does not throw
//   (`run/t8188`, `run/t8334`).
class Foo(val d: Double) extends AnyVal {
  override def toString = s"Foo($d)"
}

object Main {
  val m: Map[String, Any] = Map("s" -> "str", "i" -> 42, "l" -> List(1))
  def get[T](k: String): T = m(k).asInstanceOf[T]
  def fail[T](msg: String): T = throw new RuntimeException(msg)
  def deser[T](o: Any): T = o.asInstanceOf[T]
  def clone1[T](t: T): T = deser(t)
  def takeS(s: String) = s.length
  def fold[A, B](f: (A, => B) => B) = (b: B) => f(null.asInstanceOf[A], b)
  def id[A](a: => A): A = null.asInstanceOf[A]

  def main(args: Array[String]): Unit = {
    val i2i = (x: Int) => x + 1
    println(i2i.asInstanceOf[AnyRef => Int].apply(null))
    println(fold[Int, Int]((x, y) => x + y)(5))
    println(id[Int](???) + 1)
    val d: Double = null.asInstanceOf[Double]
    println(d)
    val f: Foo = null.asInstanceOf[Foo]
    println(f)

    val s: String = get("s")
    val i: Int = get("i")
    val l: List[Int] = get("l")
    println(s + " " + (i + 1) + " " + l.head)
    println(takeS(get("s")))
    val x: String = if (args.length == 0) get("s") else "other"
    println(x)
    get("s")
    val n: Int = try fail("boom") catch { case e: RuntimeException => e.getMessage.length }
    println(n)
    println(clone1(List(1, 2)))
  }
}
