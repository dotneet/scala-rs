// `agent/tuplepat`: the three roots behind `scala/collection/Seq.scala`'s
// `PermutationsItr` / `CombinationsItr`, in one executable file.
//
//   1. a `private[this]` member reached through the trait's **self-alias**
//      from a class nested in that trait (`self.toGenericSeq`);
//   2. `Obj[K, V](...)` where `apply` is **inherited** from a generic factory
//      trait (`object HashMap extends MapFactory[HashMap]`);
//   3. `xs.to(SomeFactory)` where the element type is an **abstract** type
//      parameter (`es.to(mutable.ArrayBuffer)` with `A` from the enclosing
//      trait).
//
// The tuple pattern definition itself was never the defect -- it is here
// because it is what carried each of the three failures into twenty-one
// "is not a member of T1/T2" reports: a component the right-hand side could
// not type leaves the accessor holding `Tuple2`'s own parameter.
import scala.collection.mutable

// --- 1, 2 and 3 together: the library's shape ------------------------------

trait Perms[A] { self =>
  def items: List[A]

  // Object-private, and read below as `self.generic` from a *nested class*.
  @inline private[this] def generic: List[A] = items
  private[this] val tag: String = "Perms"

  private class Itr {
    // A pattern definition with three components, `private[this]`, inside a
    // class nested in a generic trait -- `PermutationsItr`'s own line.
    private[this] val (elms, idxs, weights) = init()

    def render: String =
      s"${elms.size}:${idxs.length}:${weights.size}:$tag"

    private[this] def init() = {
      // 2. `mutable.HashMap[A, Int]()` is `MapFactory.apply`, inherited.
      val m = mutable.HashMap[A, Int]()
      // 1. the self-alias reaching an object-private member.
      val es = self.generic
      es.foreach(e => m.getOrElseUpdate(e, m.size))
      // 3. `to` with an abstract element type.
      (es.to(mutable.ArrayBuffer), Array.fill(es.size)(0), m)
    }
  }

  def show: String = new Itr().render
}

// --- 2 on its own: an inherited factory `apply`, with no library in sight ---

trait TinyFactory[+CC[_, _]] {
  def from[K, V](it: List[(K, V)]): CC[K, V]
  def apply[K, V](elems: (K, V)*): CC[K, V] = from(elems.toList)
}
final class TinyMap[K, V](val pairs: List[(K, V)]) {
  def size: Int = pairs.size
}
object TinyMap extends TinyFactory[TinyMap] {
  def from[K, V](it: List[(K, V)]): TinyMap[K, V] = new TinyMap[K, V](it)
}
// A *non*-higher-kinded parent parameter reaches the same redirect.
trait TinyBox[+C] {
  def mk: C
  def apply[K](): C = mk
}
final class Boxed { def label: String = "boxed" }
object BoxedOf extends TinyBox[Boxed] { def mk: Boxed = new Boxed }

class Uses[A] {
  // Explicit type arguments on the *object*, `apply` supplied by the parent.
  def two(a: A, b: A): Int = TinyMap[A, Int]((a, 1), (b, 2)).size
  def none: Int = TinyMap[A, Int]().size
  def boxed: String = BoxedOf[A]().label
}

// --- the pattern definition itself, in the shapes SLS 4.1 allows -----------

object Pat {
  // Top level of a template, no annotation.
  val (top1, top2) = ("a", 1)
  // Irrefutable but not a tuple.
  val Some(inner) = Option(42)
  // Nested, and mixing a literal that must still match.
  val (l, (m, n)) = (1, (2, 3))
  // A `var` pattern definition, assigned afterwards.
  var (v1, v2) = (10, 20)
  // Object-private accessors, read from a nested object.
  private[this] val (p1, p2) = (7, "seven")
  object Reader { def read: String = s"$p1$p2" }
  // The right-hand side is evaluated once, not once per bound name.
  var effects = 0
  def rhs(): (Int, Int) = { effects += 1; (1, 2) }
  val (e1, e2) = rhs()
}

object Main {
  def main(args: Array[String]): Unit = {
    val p = new Perms[String] { def items: List[String] = List("a", "b", "a") }
    println(p.show)
    val u = new Uses[String]
    println(u.two("x", "y"))
    println(u.none)
    println(u.boxed)
    println(s"${Pat.top1}${Pat.top2}")
    println(Pat.inner)
    println(s"${Pat.l}${Pat.m}${Pat.n}")
    Pat.v1 = 11
    println(s"${Pat.v1}${Pat.v2}")
    println(Pat.Reader.read)
    println(s"${Pat.e1}${Pat.e2}:${Pat.effects}")
  }
}
