// Every line here is an error nsc 2.13.16 gives for the same source: the
// higher-kinded `BuildFrom` match must not become "anything goes".
import scala.collection.BuildFrom

object Main {
  def main(args: Array[String]): Unit = {
    // `buildFromIterableOps` builds the receiver's own collection, so a
    // `List` receiver cannot produce a `Vector`.
    val v: Vector[String] = List("a").lazyZip(List(1)).map((s, n) => s * n)
    println(v)

    // Same, spelled as an explicit summon: `CC[A]` is `List[String]`.
    val ev = implicitly[BuildFrom[List[Int], String, Vector[String]]]
    println(ev)

    // An `Int` is no collection at all.
    val n = implicitly[BuildFrom[Int, String, List[String]]]
    println(n)
  }
}
