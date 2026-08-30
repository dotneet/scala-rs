// `scala.collection.mutable` maps, sets and buffers: the companion factories,
// the `update` sugar and the `Growable` / `Shrinkable` operators.
//
// The factory `apply` used to be inferred as the *immutable* collection of
// the same simple name, so `mutable.Set(1, 2, 3) += 4` reported that `+=` is
// not a member of `Set[Int]` -- and `s -= 2` then reported a second,
// misleading "does not convert to assignment" on top of it.
import scala.collection.mutable

object Main {
  def main(args: Array[String]): Unit = {
    // mutable.Map: `m(k) = v` is `m.update(k, v)` (SLS 6.15).
    val m = mutable.Map[String, Int]()
    m("a") = 1
    m += ("b" -> 2)
    m.update("c", 3)
    m.getOrElseUpdate("d", 4)
    m ++= List("e" -> 5)
    m.remove("a")
    m -= "b"
    m --= List("c")
    println(m.toList.sorted)
    println(m.contains("d"))

    // Built by the varargs companion, not `empty`: this is the shape that
    // used to come back immutable.
    val m2 = mutable.Map("x" -> 1)
    m2 += ("y" -> 2)
    m2("z") = 3
    println(m2.toList.sorted)

    val hm = mutable.HashMap("k" -> 1)
    hm("j") = 2
    hm -= "k"
    println(hm.toList.sorted)

    val lhm = mutable.LinkedHashMap("x" -> 1)
    lhm("y") = 2
    lhm += ("z" -> 3)
    println(lhm.toList)

    // mutable.Set
    val s = mutable.Set(1, 2, 3)
    s += 4
    s -= 2
    s ++= List(5)
    s --= List(3)
    println(s.toList.sorted)
    println(s.contains(4))
    val s2 = mutable.Set[Int]()
    s2 += 9
    println(s2.toList)

    val hs = mutable.HashSet(1, 2)
    hs += 3
    hs -= 1
    println(hs.toList.sorted)

    val lhs = mutable.LinkedHashSet(3, 1, 2)
    lhs += 4
    lhs -= 1
    println(lhs.toList)

    // Buffers, through the `Buffer` interface as well as the concrete class.
    val ab = mutable.ArrayBuffer(1, 2, 3)
    ab += 4
    ab ++= List(5)
    ab -= 1
    ab --= List(2)
    ab(0) = 30
    println(ab.toList)

    val lb = mutable.ListBuffer(1, 2, 3)
    lb += 4
    lb -= 1
    lb(0) = 20
    println(lb.toList)

    val buf: mutable.Buffer[Int] = mutable.ArrayBuffer(1, 2, 3)
    buf += 4
    buf -= 1
    buf ++= List(9)
    buf --= List(2)
    buf(0) = 7
    println(buf.toList)

    println(mutable.Map.empty[String, Int].toList)
    println(mutable.Set.empty[Int].toList)

    // A map of maps: the receiver of the outer `update` is an `apply`.
    val nested = mutable.Map[String, mutable.Map[String, Int]]()
    nested("outer") = mutable.Map[String, Int]()
    nested("outer")("inner") = 42
    println(nested("outer").toList)
  }
}
