// A class with a single constructor is not an overload: nsc types each
// argument against that constructor's formal parameter, and the parameter is
// what instantiates a polymorphic argument. `new Node(Array.empty, …)` builds
// an `Object[]` for an `Array[Any]` parameter and an `int[]` for an
// `Array[Int]` one; `Array(x)` for an `Array[Any]` parameter searches
// `ClassTag[Any]`. Typed with no expected type, both searched a tag for a
// variable nothing had solved: `No ClassTag available for T` (HashMap.scala,
// HashSet.scala in scala/scala's `src/library`).
class Node[A](val content: Array[Any], val hashes: Array[Int], val n: Int)
class Plain(val content: Array[Any], val hashes: Array[Int])
class Poly[A](val xs: Array[A], val tag: Array[Any])
class Dflt(val a: Array[Any], val n: Int = 3)
class Wid(val l: Long, val d: Double)
class ByN(x: => Array[Any]) { def get: Array[Any] = x }
class Rep(val xs: Array[Int]*)
class Outer[T] { class In(val a: Array[Any], val t: Int) }
case class CC(a: Array[Any], b: Array[Long])
class Opt(val o: Option[Array[Int]])
class K(val x: Long)
class Fl(val f: Float)
class Q(val xs: List[Any])
class Z(val a: Any)
class M(val m: Map[String, Int])
class Fn(val f: Int => Int)
class Ch(val c: Char)
class Arr2(val a: Array[Array[Any]])

object Main {
  def name(a: AnyRef): String = a.getClass.getSimpleName
  def mk[A](x: A, y: A): Node[A] = new Node[A](Array(x, y), Array(1, 2), 2)
  def mk2[A](x: A): Node[A] = new Node(Array(x), Array(3), 1)
  def main(args: Array[String]): Unit = {
    val e = new Node[String](Array.empty, Array.empty, 0)
    println(name(e.content) + " " + name(e.hashes))
    val m = mk("a", "b")
    println(name(m.content) + " " + m.content.mkString(","))
    println(name(mk2(1).content))
    val p = new Plain(Array.empty, Array.empty)
    println(name(p.content) + " " + name(p.hashes))
    val po = new Poly(Array(1, 2), Array.empty)
    println(name(po.xs) + " " + name(po.tag))
    val po2 = new Poly[String](Array.empty, Array("x"))
    println(name(po2.xs) + " " + name(po2.tag))
    val d = new Dflt(Array.empty)
    println(name(d.a) + d.n)
    val w = new Wid(1, 2)
    println(w.l + " " + w.d)
    println(name(new ByN(Array.empty).get))
    println(new Rep(Array.empty, Array(1)).xs.map(name).mkString(","))
    val o = new Outer[String]
    val in = new o.In(Array.empty, 5)
    println(name(in.a) + in.t)
    val c = CC(Array.empty, Array.empty)
    println(name(c.a) + " " + name(c.b))
    val c2 = new CC(Array(1), Array(2))
    println(name(c2.a) + " " + name(c2.b))
    println(new Opt(Some(Array.empty)).o.map(name))
    println(new Opt(None).o)
    println(new K(Int.MaxValue + 1).x)
    println(new K('a').x)
    println(new Fl(1).f)
    println(new Q(List(1, "a")).xs)
    println(new Z(1).a)
    println(new M(Map.empty).m)
    println(new Fn(_ + 1).f(2))
    println(new Ch(65).c)
    println(new Arr2(Array(Array.empty)).a.map(name).mkString)
    println(name(new Arr2(Array.empty).a))
  }
}
