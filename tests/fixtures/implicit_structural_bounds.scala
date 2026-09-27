import scala.language.reflectiveCalls

// Type classes in the shape of ScalaTest's `Length`, `Size` and `Emptiness`:
// instances over collection constructors beside instances for any class with
// the right structural member. nsc finds exactly one for each call below.
trait Len[T] { def len(t: T): Long; def tag: String }
object Len {
  implicit def seqLen[SEQ <: scala.collection.GenSeq[_]]: Len[SEQ] = new Len[SEQ] { def len(t: SEQ) = t.length.toLong; def tag = "seq" }
  implicit def jlist[JLIST <: java.util.List[_]]: Len[JLIST] = new Len[JLIST] { def len(t: JLIST) = t.size.toLong; def tag = "jlist" }
  implicit def unitLen[T <: AnyRef { def length(): Int }]: Len[T] = new Len[T] { def len(t: T) = t.length().toLong; def tag = "length()" }
  implicit def lenInt[T <: AnyRef { def length: Int }]: Len[T] = new Len[T] { def len(t: T) = t.length.toLong; def tag = "length" }
  implicit def lenLong[T <: AnyRef { def length: Long }]: Len[T] = new Len[T] { def len(t: T) = t.length; def tag = "length: Long" }
  implicit def getLen[T <: AnyRef { def getLength(): Int }]: Len[T] = new Len[T] { def len(t: T) = t.getLength().toLong; def tag = "getLength()" }
  implicit val string: Len[String] = new Len[String] { def len(t: String) = t.length.toLong; def tag = "string" }
}

trait Emp[T] { def tag: String }
object Emp {
  implicit def trav[E, TRAV[e] <: scala.collection.GenTraversable[e]]: Emp[TRAV[E]] = new Emp[TRAV[E]] { def tag = "trav" }
  implicit def jmap[K, V, JMAP[k, v] <: java.util.Map[k, v]]: Emp[JMAP[K, V]] = new Emp[JMAP[K, V]] { def tag = "jmap" }
  implicit def isEmpty[T <: AnyRef { def isEmpty: Boolean }]: Emp[T] = new Emp[T] { def tag = "isEmpty" }
}

class Box { def length: Int = 7 }
class UnitBox { def length(): Int = 8 }
class LongBox { def length: Long = 9L }
class JBox { def getLength(): Int = 10 }
class VBox { val isEmpty: Boolean = true }

object Main {
  def l[T](t: T)(implicit ev: Len[T]) = s"${ev.tag}:${ev.len(t)}"
  def e[T](t: T)(implicit ev: Emp[T]) = ev.tag
  def main(args: Array[String]): Unit = {
    println(l(List(1, 2, 3)))
    println(l(Vector(1)))
    println(l("abcd"))
    val jl = new java.util.ArrayList[Int]; jl.add(1)
    println(l(jl))
    println(l(new Box))
    println(l(new UnitBox))
    println(l(new LongBox))
    println(l(new JBox))
    println(e(List(1)))
    println(e(Map(1 -> 2)))
    println(e(new java.util.HashMap[String, Int]))
    println(e(new VBox))
  }
}
