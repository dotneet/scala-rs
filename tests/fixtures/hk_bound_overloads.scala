// Overloads told apart by the bound of a type-constructor parameter, in the
// shape of ScalaTest's `Inspectors.forAll`, and an infix call whose tuple
// argument fills a clause ending in a repeated parameter (`contain allOf`).
object Ins {
  def forAll[E, C[_], R](xs: C[E])(fun: E => R)(implicit ev: C[E] <:< Iterable[E]): String = "coll " + ev(xs).map(fun).toList
  def forAll[K, V, MAP[k, v] <: scala.collection.GenMap[k, v], R](xs: MAP[K, V])(fun: ((K, V)) => R): String = "map " + xs.map(fun).toList
  def forAll[K, V, JMAP[k, v] <: java.util.Map[k, v], R](xs: JMAP[K, V])(fun: java.util.Map.Entry[K, V] => R): String = "jmap " + xs.size
  def forAll[R](xs: String)(fun: Char => R): String = "string " + xs.toList.map(fun)
}

class Words {
  def allOf(first: Any, second: Any, rest: Any*): String = s"$first $second ${rest.mkString(",")}".trim
  def two(a: Any, b: Any): String = s"$a|$b"
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Ins.forAll(List(1, 2)) { x => x + 1 })
    println(Ins.forAll(Vector("a")) { x => x.length })
    println(Ins.forAll(Map(1 -> "a")) { case (k, v) => k + v.length })
    val jm = new java.util.HashMap[String, Int]; jm.put("k", 1)
    println(Ins.forAll(jm) { e => e.getValue })
    println(Ins.forAll("ab") { c => c.toInt })
    val w = new Words
    println(w allOf (1, 2))
    println(w allOf (1, 2, 3))
    println(w two (1, 2))
  }
}
