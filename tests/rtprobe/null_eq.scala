// `==` with null on either side (never NPE), null unboxed to primitives,
// null in string concatenation and collections, and Option(null).
object Main {
  case class C(s: String)
  def asInt(a: Any): Int = a.asInstanceOf[Int]
  def gen[T](t: T): T = t
  def main(args: Array[String]): Unit = {
    val s: String = null; val t: String = "x"; val o: AnyRef = null; val a: Any = null
    println(s == null); println(null == s); println(s == t); println(t == s); println(s != t); println(o == s); println(a == null)
    println(s eq null); println(s ne t)
    val c: C = null
    println(c == C("x")); println(C("x") == c); println(C(null) == C(null))
    println(null.asInstanceOf[Int] + " " + null.asInstanceOf[Boolean] + " " + null.asInstanceOf[Double] + " " + null.asInstanceOf[Char].toInt + " " + null.asInstanceOf[Long])
    println(asInt(null) + " " + gen[String](null) + " " + (gen[Any](null) == null))
    println("str:" + s + " " + s"interp:$s" + " " + String.valueOf(o))
    println(Option(s) + " " + Option(t) + " " + Option(null: Integer).isDefined + " " + Some(null: String).isDefined)
    println(List(null, "a").count(_ == null) + " " + List[String](null, "b").contains(null) + " " + Map("k" -> (null: String)).get("k"))
    val boxed: java.lang.Integer = null
    println(boxed == null); println(boxed == 0 || true)
    val jl: java.lang.Long = 5L
    println(jl == 5); println(jl == 5L); println(jl.equals(5)); println(jl.equals(5L))
    val arr = new Array[String](1)
    println(arr(0) == null); println(arr(0) + "!")
    val nullFn: Int => Int = null
    println(nullFn == null)
    try { println(s.length) } catch { case _: NullPointerException => println("npe on s.length") }
    try { println(c.s) } catch { case _: NullPointerException => println("npe on c.s") }
    println(s match { case null => "matched null"; case _ => "not" })
    println((null: Any) match { case _: String => "string"; case _ => "not a string" })
    println(a.isInstanceOf[String] + " " + a.isInstanceOf[Any] + " " + (s: Any).isInstanceOf[AnyRef])
    println(s.## + " " + (null: Any).hashCode.equals(0).toString.length)
  }
}
