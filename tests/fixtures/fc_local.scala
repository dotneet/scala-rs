// The same missing cast, seen through a *local*: the factory result is stored
// in a `val`, so the frame records the slot as `scala/collection/SeqOps` and
// the verifier rejects the first call that wants the collection itself.
// Nothing here fails when the factory call is used once and thrown away --
// binding it and then calling several methods is what exposes it.
import scala.collection.immutable.{LazyList, Queue}
import scala.collection.mutable.{ArrayBuffer, ListBuffer}

object Main {
  def main(args: Array[String]): Unit = {
    val v = Vector.tabulate(5)(i => i * i)
    println(v)
    println(v.updated(0, 99))
    println(v.slice(1, 3), v.takeRight(2), v.dropRight(3))

    val vf = Vector.fill(3)(7)
    println(vf.updated(0, 1), vf.reverse)

    val vc = Vector.concat(Vector(1), Vector(2))
    println(vc.head, vc.updated(0, 9))

    val vi = Vector.iterate(1, 3)(_ + 1)
    println(vi.head, vi.reverse)

    val l = List.tabulate(5)(i => i * i)
    println(l.updated(0, 99), l.slice(1, 3), l.takeRight(2), l.dropRight(3))

    val lf = List.fill(3)(7)
    println(lf ::: List(9), lf.reverse)

    val s = Seq.tabulate(5)(i => i * i)
    println(s.updated(0, 99), s.slice(1, 3))

    val is = IndexedSeq.tabulate(5)(i => i * i)
    println(is.updated(0, 99), is.slice(1, 3))

    val st = Set.tabulate(3)(i => i)
    println(st.size, st - 0)

    val m = Map.from(Seq(1 -> "a"))
    println(m.size, m + (2 -> "b"))

    val ll = LazyList.tabulate(3)(i => i)
    println(ll.head, ll.take(2).toList)

    val q = Queue.fill(2)(5)
    println(q.size, q.enqueue(1))

    val ab = ArrayBuffer.tabulate(3)(i => i)
    println(ab.size, ab.reverse)

    val lb = ListBuffer.fill(2)(5)
    println(lb.size, lb.reverse)

    // Widened to a supertype through a second `val`, and returned from a
    // method: both have to keep the narrow slot type usable.
    val widened: Seq[Int] = v
    println(widened.size, v.updated(0, 9))
    val fromDef = mkVector()
    println(fromDef.updated(0, 9), fromDef.slice(0, 1))
  }

  def mkVector(): Vector[Int] = Vector.tabulate(3)(i => i)
}
