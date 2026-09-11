// Numeric typing that changes what runs: numeric widening is weak
// conformance and counts toward overload applicability; branches without an
// expected type join by weak lub (and are widened), but a defined expected
// type such as `println`'s `Any` keeps them as they are; a negative literal
// binds tighter than a selection; `Int.box` is a primitive, not the library's
// `???`; and unboxing `null` gives the primitive's zero.
object Main {
  def f(x: Int) = "Int"; def f(x: Long) = "Long"; def f(x: Double) = "Double"; def f(x: Any) = "Any"
  def gen[T](t: T) = "generic"; def gen(i: Int) = "specific-Int"
  def d(x: Double) = "D"; def d(x: Any) = "A"
  def show(x: Any): String = x.toString
  def asInt(a: Any): Int = a.asInstanceOf[Int]
  def main(args: Array[String]): Unit = {
    val b: Byte = 1; val s: Short = 2; val ch = 'c'; val fl = 1.5f
    println(List(f(b), f(s), f(ch), f(fl), f(1L), f("x")).mkString(" ") + " " + gen(b) + " " + d(1) + " " + d("s"))
    println(ch)
    println(1 / 3.0f)
    val n = args.length
    val a = n match { case 0 => 1; case _ => 2.0 }
    val c = if (n == 0) 'a' else 1
    val l = List(1, 2) match { case h :: _ => h; case Nil => 0L }
    val t = try 1 catch { case _: Exception => 2.0 }
    println(s"$a $c $l $t")
    println(f(if (n == 0) 1 else 2L) + " " + f(n match { case 0 => 'c'; case _ => 3 }))
    println(if (n == 0) 1 else 2.0)
    println(show(if (n == 0) 1 else 2.0))
    val x: Any = if (n == 0) 1 else 2.0
    println(x)
    println(List(if (n == 0) 1 else 2.0))
    println(-5.abs + " " + -5.0.abs + " " + (- 5.abs) + " " + -1.toString + " " + (-2).abs)
    val boxed: java.lang.Integer = Int.box(5)
    println(boxed + " " + Double.box(1.5) + " " + Long.box(3L) + " " + Char.box('c') + " " + Int.unbox(boxed) + " " + Long.unbox(Long.box(7L)))
    println(String.format("%s=%d", "k", Int.box(5)))
    println(null.asInstanceOf[Int] + " " + null.asInstanceOf[Boolean] + " " + null.asInstanceOf[Double] + " " + asInt(null) + " " + asInt(5))
    try println(asInt("s")) catch { case _: ClassCastException => println("CCE") }
  }
}
