// The same erasure hole across the rest of the factories, and a guard for the
// case that already worked (`TreeMap - key` is declared to return `Map` on the
// JVM while the typer narrows it to `TreeMap`): both are "the descriptor's
// return type is not the erasure of the result type", and one rule has to
// cover both.
import scala.collection.immutable.{LazyList, Queue, SortedMap, SortedSet, TreeMap, TreeSet}
import scala.collection.mutable.{ArrayBuffer, ListBuffer}

object Main {
  def main(args: Array[String]): Unit = {
    println(TreeMap(1 -> "a", 2 -> "b").-(1).size)
    println(TreeSet(1, 2, 3).-(1).size)
    println(SortedSet(3, 1, 2).toList)
    println(SortedMap(2 -> "b", 1 -> "a").toList)

    println(ArrayBuffer.tabulate(3)(i => i).size)
    println(ListBuffer.tabulate(3)(i => i).size)
    println(ArrayBuffer.fill(2)(5).head)
    println(ListBuffer.fill(2)(5).head)
    println(LazyList.tabulate(3)(i => i).toList)
    println(Queue.fill(2)(5).size)

    println(Iterator.tabulate(3)(i => i).toList)
    println(Iterator.fill(2)(5).toList)

    println(List.fill(2)(5).sum)
    println(List.fill(2)(5).mkString(","))
    println(List.tabulate(3)(i => i).sorted)
    println(List.fill(2)(5).toArray.length)
    println(List.fill(2)(5).iterator.toList)
    println(List.fill(2)(5).zip(List(1, 2)))

    println(Seq.fill(2)(5) ++ Seq(9))
    println(Set.fill(2)(5) + 9)
    println(Set.concat(Set(1), Set(2)).size)
    println(Vector.concat(Vector(1), Vector(2)).size)
    println(Seq.concat(Seq(1), Seq(2)).size)
    println(Vector.tabulate(3)(i => i) ++ Vector(9))
    println(Vector.fill(2)(5).last)
  }
}
