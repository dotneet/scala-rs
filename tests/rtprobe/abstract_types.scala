// Abstract type members, path-dependent types, type refinements, and
// generic bounded type parameters (whose use needs a checkcast after erasure).
object Main {
  trait Container { type Elem; def elems: List[Elem]; def first: Elem = elems.head }
  class IntBag(val elems: List[Int]) extends Container { type Elem = Int }
  class StrBag(val elems: List[String]) extends Container { type Elem = String }
  def firstOf(c: Container): c.Elem = c.first

  trait Animal { type Food <: AnyRef; def eat(f: Food): String }
  class Grass { override def toString = "grass" }
  class Cow extends Animal { type Food = Grass; def eat(f: Grass) = "cow eats " + f }

  class Graph { class Node(val id: Int) { def connect(o: Node): String = s"$id->${o.id}" }; def node(i: Int) = new Node(i) }

  abstract class Buffer { type T; val items: scala.collection.mutable.ListBuffer[T] = scala.collection.mutable.ListBuffer.empty; def add(t: T): this.type = { items += t; this } }
  class StringBuf extends Buffer { type T = String }

  def maxOf[T <: Comparable[T]](a: T, b: T): T = if (a.compareTo(b) >= 0) a else b
  def lenOf[S <: CharSequence](s: S): Int = s.length
  def headStr[L <: List[String]](l: L): String = l.head.toUpperCase
  class Holder[T <: Number](val n: T) { def dbl: Double = n.doubleValue * 2 }
  def firstUpper[A <: String](xs: List[A]): String = xs.head.toUpperCase
  trait Pet { def name: String }
  case class Dog(name: String) extends Pet
  def names[P <: Pet](ps: List[P]): List[String] = ps.map(_.name)
  def pick[P <: Pet](p: P, q: P): P = if (p.name < q.name) p else q
  def refine(x: { type T = Int; def v: T }): Int = x.v + 1

  def main(args: Array[String]): Unit = {
    val ib = new IntBag(List(4, 5)); val sb = new StrBag(List("x", "y"))
    val i: Int = firstOf(ib); val s: String = firstOf(sb)
    println(i + 1); println(s + "!")
    println(new Cow().eat(new Grass))
    val g = new Graph; val n1 = g.node(1); val n2 = g.node(2)
    println(n1.connect(n2))
    println(new StringBuf().add("a").add("b").items.mkString)
    println(maxOf("pear", "apple") + " " + maxOf(Integer.valueOf(3), Integer.valueOf(9)) + " " + lenOf("abcd") + " " + lenOf(new java.lang.StringBuilder("xy")))
    println(headStr(List("hd", "tl")) + " " + new Holder(Integer.valueOf(21)).dbl + " " + new Holder(java.lang.Double.valueOf(1.25)).dbl)
    println(firstUpper(List("lower")) + " " + names(List(Dog("rex"), Dog("ace"))) + " " + pick(Dog("b"), Dog("a")).name)
    val d: Dog = pick(Dog("z"), Dog("y"))
    println(d.name.length)
    println(refine(new { type T = Int; def v = 41 }))
  }
}
