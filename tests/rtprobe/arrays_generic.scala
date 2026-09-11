// Generic arrays through ClassTag, Array[T] in generic methods (which must
// use the runtime helpers), arrays of references viewed as Array[Any], and
// element types after erasure.
import scala.reflect.ClassTag
object Main {
  def mk[T: ClassTag](n: Int, v: T): Array[T] = Array.fill(n)(v)
  def first[T](a: Array[T]): T = a(0)
  def setFirst[T](a: Array[T], v: T): Unit = a(0) = v
  def len[T](a: Array[T]): Int = a.length
  def rev[T: ClassTag](a: Array[T]): Array[T] = { val r = new Array[T](a.length); for (i <- a.indices) r(i) = a(a.length - 1 - i); r }
  def swap[T](a: Array[T], i: Int, j: Int): Unit = { val t = a(i); a(i) = a(j); a(j) = t }
  def toAny[T](a: Array[T]): String = a.map(x => x: Any).mkString(",")
  def cls[T: ClassTag]: String = implicitly[ClassTag[T]].runtimeClass.getName
  def main(args: Array[String]): Unit = {
    val ai = mk(3, 7); val ad = mk(2, 1.5); val as = mk(2, "s"); val ac = mk(2, 'c'); val ab = mk(1, true)
    println(ai.getClass.getSimpleName + " " + ad.getClass.getSimpleName + " " + as.getClass.getSimpleName + " " + ac.getClass.getSimpleName + " " + ab.getClass.getSimpleName)
    println(first(ai) + first(ad) + first(as) + first(ac))
    setFirst(ai, 100); setFirst(ad, 2.5); setFirst(as, "t"); setFirst(ac, 'z')
    println(ai.toList + " " + ad.toList + " " + as.toList + " " + ac.toList)
    println(len(ai) + len(as) + len(Array(1L, 2L, 3L)))
    println(rev(Array(1, 2, 3)).toList + " " + rev(Array("a", "b")).toList + " " + rev(Array(1.0, 2.0)).toList)
    val sw = Array(1, 2, 3); swap(sw, 0, 2); println(sw.toList)
    val sw2 = Array("x", "y"); swap(sw2, 0, 1); println(sw2.toList)
    val sw3 = Array('a', 'b', 'c'); swap(sw3, 0, 1); println(sw3.mkString)
    println(toAny(Array(1, 2)) + " " + toAny(Array(true)) + " " + toAny(Array(1.5f)))
    println(cls[Int] + " " + cls[String] + " " + cls[Array[Int]] + " " + cls[List[Int]] + " " + cls[Unit])
    val objs: Array[Any] = Array(1, "two", 3.0)
    println(objs.map(_.getClass.getSimpleName).toList)
    val refs: Array[AnyRef] = Array("a", List(1))
    println(refs.length)
    val strsAsObj: Array[_ <: AnyRef] = Array("p", "q")
    println(strsAsObj.getClass.getSimpleName + " " + strsAsObj.length)
    val nested: Array[Array[String]] = Array(Array("a"), Array("b", "c"))
    println(nested.map(_.length).sum + " " + nested(1)(1))
    val lst = List(1, 2, 3).toArray
    println(lst.getClass.getSimpleName + " " + lst.sum)
    val vec = Vector("x", "y").toArray
    println(vec.getClass.getSimpleName)
    val gen = Array.ofDim[String](2)
    println(gen.toList)
    val u = Array((), ())
    println(u.length + " " + u.getClass.getSimpleName)
  }
}
