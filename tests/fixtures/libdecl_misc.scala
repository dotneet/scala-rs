// Declaration and member-lookup fixes found compiling scala/scala's own
// `src/library`, each reduced to user code and dual-run against scalac.
import java.util.Arrays
import scala.util.DynamicVariable

// 1. A parent's self alias (`self =>`) is not a member. `s.self` on
//    `WrappedString(private val self: String)` read as an overload of the
//    field and `Function1.this.type`.
trait F[-A] extends (A => Int) { self =>
  def me: F[A] = self
}
final class W(private val self: String) extends F[Int] {
  def apply(i: Int): Int = i
  def same(o: Any): Boolean = o match {
    case w: W => self.startsWith(w.self, 0)
    case _    => false
  }
}
trait B0 { def b: Int = 2 }
trait A0 { self: B0 =>
  def a: Int = self.b + 1
  def anon: Int = new B0 { override def b: Int = self.b * 10 }.b
}
class AB extends A0 with B0

// 2. With no expected type a method type parameter the arguments solve to
//    `Nothing` is `Nothing`: `onError { e => throw e }` is a
//    `PartialFunction[Throwable, Nothing]`, and `try p catch onError {…}`
//    keeps `p`'s type (`sys/process/ProcessImpl.scala`).
object Catchers {
  type =?>[-A, +B] = PartialFunction[A, B]
  def onError[T](handler: Throwable => T): Throwable =?> T = {
    case e @ _ => handler(e)
  }
  class P { def exitValue(): Int = 7 }
  def run(): P = new P
}

// 3. A parameterless getter with a `name_=` beside it takes `op=`:
//    `size += 1` on `def size` + `private def size_=` (`OpenHashMap.scala`).
class Acc {
  private var size0 = 0
  def p_size0 = size0
  def p_size0_=(s: Int) = size0 = s
}
class Sized {
  private[this] var _s = 0
  override def toString = "Sized(" + _s + ")"
  def size = _s
  private def size_=(s: Int): Unit = _s = s
  def inc(): Unit = { size += 1; size -= 3; size *= 2 }
  val acc = new Acc
  acc.p_size0 += 5
}

// 4. A Java `Object` parameter matches `AnyRef`, `Object` and `Any` when
//    overriding (nsc's `ObjectTpeJava`); `JavaCollectionWrappers` overrides
//    `contains(o: Object)` / `remove(elem: AnyRef)`.
class MyList extends java.util.AbstractList[String] {
  def get(i: Int): String = "x" + i
  def size: Int = 2
  override def contains(o: Object): Boolean = o == "x1"
  override def indexOf(o: AnyRef): Int = 7
  override def remove(o: Any): Boolean = false
}
class MyMap extends java.util.AbstractMap[String, Int] {
  def entrySet(): java.util.Set[java.util.Map.Entry[String, Int]] = new java.util.HashSet()
  override def get(key: AnyRef): Int = 42
  override def containsKey(key: Object): Boolean = true
}

object Main {
  def kind(x: Any): String = x.getClass.getSimpleName
  def cumulative(i: Int): Long = i * 10L

  def main(args: Array[String]): Unit = {
    // 1
    println(new W("abc").same(new W("ab")) + " " + new W("abc").same(1))
    val ab = new AB
    println(ab.a + " " + ab.anon)

    // 2
    import Catchers._
    val first =
      try run()
      catch onError { err => throw err }
    println(first.exitValue())
    val h = onError { err => throw err }
    val y: String = try "ok" catch h
    println(y)

    // 3
    val sz = new Sized
    sz.inc()
    println(sz + " " + sz.acc.p_size0)
    val dv = new DynamicVariable[Int](3)
    dv.value += 4
    println(dv.value)

    // 4
    val l = new MyList
    println(l.contains("x1") + " " + l.contains("q") + " " + l.indexOf("z") + " " + l.remove("x0"))
    val m = new MyMap
    println(m.get("k") + " " + m.containsKey("z"))
    val jl: java.util.List[String] = l
    println(jl.contains("x1") + " " + jl.indexOf("a"))

    // 5. An `Object[]` parameter takes `Array[AnyRef]` (and `Array[Any]`):
    //    `java.util.Arrays.fill(table.asInstanceOf[Array[AnyRef]], null)`.
    val refs = Array[AnyRef]("c", "a", "b")
    Arrays.sort(refs)
    println(Arrays.toString(refs))
    Arrays.fill(refs, null)
    println(refs.toList)
    val any = Array[Any](1, "b")
    Arrays.fill(any, 3)
    println(any.toList)
    println(Arrays.deepToString(Array[AnyRef](Array[AnyRef]("x"), "y")))
    val jal = new java.util.ArrayList[String]()
    jal.add("q")
    val arr: Array[AnyRef] = jal.toArray()
    val arr2: Array[Any] = jal.toArray()
    println(arr.toList ++ arr2.toList)

    // 6. With no expected type, numeric branches meet at their weak lub
    //    (`if (c) aLong else 0` is a `Long`, not `AnyVal`).
    val c = args.length == 0
    val n = args.length
    println((if (c) cumulative(1) else 0) + 3)
    val a = if (c) 1L else 0
    println(kind(a) + " " + (a + 1))
    val d = if (c) 'a' else 1.5
    println(kind(d) + " " + d)
    val s: Short = 3
    val e = if (c) s else 'b'
    println(kind(e) + " " + e)
    val mm = n match { case 0 => 2L; case 1 => 3; case _ => 1.5f }
    println(kind(mm) + " " + mm)
    val t = n match { case 0 => 2L; case 1 => 3; case _ => throw new Exception }
    println(kind(t))
    val boxed: Any = if (!c) 1L else 0
    println(kind(boxed))
  }
}
