// Mutable collections: ArrayBuffer, ListBuffer, HashMap getOrElseUpdate,
// in-place ops, mutable Set, Queue/Stack, StringBuilder, and aliasing.
import scala.collection.mutable
object Main {
  def main(args: Array[String]): Unit = {
    val ab = mutable.ArrayBuffer(1, 2, 3)
    ab += 4; ab ++= List(5, 6); ab.insert(0, 0); ab -= 3; ab.remove(1)
    println(ab + " " + ab.length + " " + ab(2))
    ab(0) = 100; ab.update(1, 200)
    println(ab.toList + " " + ab.sum)
    val alias = ab; alias += 7
    println(ab.last + " " + (alias eq ab))
    ab.clear(); println(ab.isEmpty)
    val lb = mutable.ListBuffer.empty[String]
    lb += "a"; "b" +=: lb; lb ++= Seq("c", "d"); lb -= "c"
    println(lb.toList + " " + lb.head)
    val hm = mutable.HashMap.empty[String, Int]
    for (w <- "the cat the dog the end".split(" ")) hm(w) = hm.getOrElse(w, 0) + 1
    println(hm.toList.sorted)
    var calls = 0
    val cache = mutable.Map.empty[Int, Int]
    def memo(n: Int): Int = cache.getOrElseUpdate(n, { calls += 1; n * n })
    println(memo(3) + memo(3) + memo(4) + " calls=" + calls)
    hm.remove("the"); hm -= "cat"; hm += ("x" -> 9); hm.put("y", 10)
    println(hm.toList.sorted + " " + hm.contains("the") + " " + hm.get("x"))
    hm.transform((_, v) => v * 2); hm.filterInPlace((k, _) => k != "y")
    println(hm.toList.sorted)
    val ms = mutable.Set(1, 2); ms += 3; ms -= 1; ms ++= Set(10, 11)
    println(ms.toList.sorted + " " + ms.add(2) + " " + ms.add(99) + " " + ms.size)
    val q = mutable.Queue(1, 2); q.enqueue(3); val d = q.dequeue()
    println(d + " " + q.toList + " " + q.front)
    val st = mutable.Stack(1, 2); st.push(0); val p = st.pop()
    println(p + " " + st.toList + " " + st.top)
    val sb = new StringBuilder("ab")
    sb += 'c'; sb ++= "de"; sb.append(1).append(2.5).append(true); sb.insert(0, "<"); sb.setCharAt(1, 'A')
    println(sb.toString + " " + sb.length + " " + sb.reverse + " " + sb.indexOf("de"))
    val arr = mutable.ArrayBuffer(5, 3, 1, 4)
    println(arr.sorted + " " + arr.sortInPlace() + " " + arr)
    val tm = mutable.TreeMap(3 -> "c", 1 -> "a"); tm(2) = "b"
    println(tm.toList + " " + tm.firstKey)
    val lhm = mutable.LinkedHashMap("z" -> 1, "a" -> 2); lhm("m") = 3
    println(lhm.keys.toList)
    val bs = mutable.BitSet(1, 5, 3); bs += 64
    println(bs.toList)
    val am = mutable.ArrayBuffer.fill(3)(0)
    for (i <- am.indices) am(i) += i * 2
    println(am)
    val hs = mutable.HashSet("x"); val snapshot = hs.toSet; hs += "y"
    println(snapshot.size + " " + hs.size)
    val m2 = mutable.Map(1 -> List(1)); m2(1) ::= 0; m2(2) = Nil
    println(m2.toList.sortBy(_._1))
    val counter = mutable.Map.empty[Char, Int].withDefaultValue(0)
    "hello".foreach(c => counter(c) += 1)
    println(counter.toList.sorted)
  }
}
