// Application and argument-conformance repairs found by compiling
// scala/scala's own library (`tests/scalalib_measure.sh`). Each section is a
// reduction of one library root; the program's output is compared with scalac.

// A Java `Object[]` parameter is `Array[ObjectTpeJava]`: an `Array[AnyRef]`
// and an `Array[Any]` both fit it (`scala/Array.scala`'s
// `Arrays.fill(result.asInstanceOf[Array[AnyRef]], ())`).
object JavaArrays {
  def run(): Unit = {
    val a = new Array[AnyRef](3)
    java.util.Arrays.fill(a, null)
    println(a.toList)
    java.util.Arrays.fill(a, ())
    println(a.toList)
    java.util.Arrays.fill(a, 0, 1, "x")
    println(a.toList)
    val an = new Array[Any](2)
    java.util.Arrays.fill(an, 7)
    println(an.toList)
    println(java.util.Arrays.asList(an: _*))
  }
}

// An earlier *selection* of an overload group on another instantiation
// (`b.add` on `MB[K1, V1]`) must not decide what the bare `add` inside
// `MB[K, V]` means (`immutable/HashMap.scala`'s `HashMapBuilder.addAll`).
object UseMB {
  def make[K1, V1](k: K1, v: V1): List[(K1, V1)] = {
    val b = new MB[K1, V1]
    b.add(k, v)
    b.add((k, v))
    b.result()
  }
}

final class MB[K, V] {
  private var acc = List.empty[(K, V)]
  def add(elem: (K, V)): this.type = { acc = elem :: acc; this }
  def add(key: K, value: V): this.type = add((key, value))
  def addAll(xs: IterableOnce[(K, V)]): this.type = {
    val it = xs.iterator
    while (it.hasNext) add(it.next())
    this
  }
  def result(): List[(K, V)] = acc.reverse
}

// `x += 1` on a getter with a setter beside it is `x = x + 1`
// (`mutable/OpenHashMap.scala`, `mutable/PriorityQueue.scala`).
class SizeBase { protected var size0 = 0; def size: Int = size0 }

class Access extends SizeBase {
  def p_size0 = size0
  def p_size0_=(s: Int) = size0 = s
}

class Counted extends SizeBase {
  private[this] var _size = 0
  override def size = _size
  private[this] def size_=(s: Int): Unit = _size = s
  def bump(): Unit = { size += 1; size += 2; size -= 1 }
}

// The branches of an `if` / `match` with no expected type meet at their
// numeric lub (`scala/jdk/*Accumulator.scala`).
object NumericLub {
  def cumulative(i: Int): Long = 10L * i
  def f(h: Int): Long = cumulative(h) - (if (h > 0) cumulative(h - 1) else 0)
  def g(h: Int, index: Int): Long = (if (h > 0) cumulative(h - 1) else 0) + index
  def m(h: Int) = h match { case 0 => 1.5f; case 1 => 'c'; case _ => 7 }
  def boxed(b: Boolean): Any = { val v = if (b) 1L else 0; v }
  def anyPt(b: Boolean): Any = if (b) 1L else 0
  def charShort(b: Boolean) = { val c: Char = 'a'; val s: Short = 3; if (b) c else s }
  def run(): Unit = {
    println(List(f(0), f(3), g(0, 7), g(2, 7)))
    println(List(m(0), m(1), m(2)))
    println(boxed(false).getClass.getName + " " + anyPt(false).getClass.getName)
    println((charShort(true): Any).getClass.getName + " " + charShort(false))
  }
}

// `var x: A = _` is the zero of a type parameter (`Iterator.scala`).
class Slot[A] {
  private[this] var hd: A = _
  var elem: A = _
  def set(a: A): Unit = { hd = a; elem = a }
  def get: A = hd
}

trait Holder { type T; var t: T = _ }

// A single constructor's parameter types are its arguments' expected types:
// `Array(k, v)` for an `Array[Any]` is an `Array[Any]`
// (`immutable/HashMap.scala`'s `BitmapIndexedMapNode`).
class Node(val content: Array[Any], val hashes: Array[Int])
class GNode[A](val xs: Array[Any], val a: A)
class Narrow(val b: Byte)
class Overloaded(val tag: String) {
  def this(i: Int) = this("int")
  def this(l: Long) = this("long")
}

object Main {
  def mk[K, V](k: K, v: V): Node = new Node(Array(k, v), Array(1))
  def mk2(a: AnyRef, b: AnyRef): Node = new Node(Array(a, b), Array(2))

  def main(args: Array[String]): Unit = {
    JavaArrays.run()
    println(UseMB.make(1, "a"))
    println(new MB[Int, String].addAll(List(2 -> "b")).result())
    val a = new Access
    a.p_size0 += 1
    a.p_size0 += 5
    println(a.p_size0)
    val c = new Counted
    c.bump()
    println(c.size)
    NumericLub.run()
    val s = new Slot[Int]
    s.set(3)
    println(s.get + s.elem)
    println(new Slot[String].get == null)
    val h = new Holder { type T = String }
    println(h.t)
    h.t = "x"
    println(h.t)
    println(mk(1, "a").content.toList)
    println(mk2("x", "y").content.toList)
    val g = new GNode[String](Array("a", 1), "x")
    println(g.xs.toList + " " + g.a)
    println(new Narrow(1).b)
    println(new Overloaded(1).tag)
  }
}
