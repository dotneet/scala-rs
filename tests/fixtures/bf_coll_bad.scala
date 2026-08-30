// What the `BuildFrom` narrowing must NOT swallow. Every line here is an
// error real scalac 2.13.16 gives for the same source.
import scala.collection.mutable.ArrayBuffer

object Main {
  def main(args: Array[String]): Unit = {
    // The lambda does not return a pair, so the result is an `Iterable`, not
    // a `Map` -- the receiver being a `Map` does not make it one.
    val bad1: Map[String, Int] = Map("a" -> 1).map { case (_, v) => v }
    // `to(ArrayBuffer)` is an `ArrayBuffer`, not a `List`.
    val bad2: List[Int] = List(1, 2).to(ArrayBuffer)
    // `groupMapReduce`'s value type is what the *second* clause returns.
    val bad3: Map[String, String] = List(("x", 1)).groupMapReduce(_._1)(_._2)(_ + _)
    println(bad1)
    println(bad2)
    println(bad3)
  }
}
