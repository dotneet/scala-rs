// The half that says the rule is a *restriction*.
//
// `SortedMapOps.map[K2, V2](f)(implicit ordering: Ordering[K2])` is chosen
// over `MapOps.map` because it is the more derived declaration -- not
// unconditionally. Without an `Ordering` for the new key type the witness
// cannot be filled in, nsc falls back to `MapOps.map`, and the result really
// is an unordered `Map`. Both lines below are rejected by real scalac 2.13.16
// ("No implicit Ordering[Key] found to build a SortedMap[Key, String]"), and a
// fix that made the sorted overload win unconditionally would compile them and
// hand back a collection with no ordering at run time.
//
// `SortedSet`'s `map` is not among them: it is still supplied by the prelude's
// `Set.map` and narrowed without any witness at all, so this compiler accepts
// `s.map(i => Key(i)): SortedSet[Key]` and throws at run time. That is a
// separate defect, recorded in `docs/cats.md`, and putting it here would only
// pin the wrong behaviour.
import scala.collection.immutable.SortedMap

final case class Key(n: Int)

object Main {
  def main(args: Array[String]): Unit = {
    val m: SortedMap[Int, String] = SortedMap(1 -> "a", 2 -> "b")
    val bad1: SortedMap[Key, String] = m.map { case (k, v) => (Key(k), v) }
    println(bad1)
    val bad2: SortedMap[Key, String] = m.collect { case (k, v) => (Key(k), v) }
    println(bad2)
  }
}
