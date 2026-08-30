// `scala.collection.mutable` sequence and sorted collections whose companion
// `apply` is inherited from `IterableFactory` / `SortedIterableFactory` /
// `EvidenceIterableFactory`. Reading the classfile signature for those gave a
// non-repeated `Seq[A]` parameter and an abstract `CC` result, so every
// `Queue[Int]()` reported `no matching overload for (Seq[Int])CC with
// arguments ()` -- the empty list included.
import scala.collection.mutable

object Main {
  def main(args: Array[String]): Unit = {
    val q = mutable.Queue[Int]()
    q.enqueue(1)
    q.enqueue(2)
    q.enqueue(3)
    println(q.dequeue())
    println(q.head)
    println(q.size)
    println(q.toList)
    q += 4
    q ++= List(5)
    q -= 5
    println(q.toList)
    println(mutable.Queue(1, 2, 3).toList)
    println(mutable.Queue.empty[Int].isEmpty)
    println(new mutable.Queue[Int]().isEmpty)

    val st = mutable.Stack[Int]()
    st.push(1)
    st.push(2)
    println(st.pop())
    println(st.top)
    println(st.size)
    st += 7
    println(st.toList)
    println(mutable.Stack(1, 2, 3).toList)
    println(mutable.Stack.empty[Int].isEmpty)
    println(new mutable.Stack[Int]().isEmpty)

    val d = mutable.ArrayDeque[Int]()
    d += 1
    d.append(2)
    d.prepend(0)
    println(d.toList)
    println(mutable.ArrayDeque(1, 2, 3).toList)
    println(new mutable.ArrayDeque[Int]().isEmpty)

    val p = mutable.PriorityQueue[Int]()
    p.enqueue(3)
    p.enqueue(9)
    p += 1
    p ++= List(5)
    println(p.dequeue())
    println(mutable.PriorityQueue(1, 9, 4).dequeue())
    println(mutable.PriorityQueue.empty[Int].isEmpty)
    println(new mutable.PriorityQueue[Int]().isEmpty)

    val ts = mutable.TreeSet[Int]()
    ts += 3
    ts += 1
    ts ++= List(2)
    ts -= 2
    println(ts.toList)
    println(mutable.TreeSet(3, 1, 2).toList)
    println(mutable.TreeSet.empty[Int].isEmpty)
    println(new mutable.TreeSet[Int]().isEmpty)

    val tm = mutable.TreeMap[String, Int]()
    tm("b") = 2
    tm("a") = 1
    tm += ("c" -> 3)
    tm -= "c"
    println(tm.toList)
    println(mutable.TreeMap("x" -> 1, "a" -> 2).toList)
    println(mutable.TreeMap.empty[String, Int].isEmpty)
    println(new mutable.TreeMap[String, Int]().isEmpty)

    val as = mutable.ArraySeq(1, 2, 3)
    as(0) = 9
    println(as(0))
    println(as.length)
    println(as.mkString(","))
    println(mutable.ArraySeq.empty[Int].length)

    val sb = new mutable.StringBuilder()
    sb ++= "ab"
    sb.append("cd")
    println(sb.toString)
    val sb2 = mutable.StringBuilder.newBuilder
    sb2 ++= "zz"
    println(sb2.toString)
  }
}
