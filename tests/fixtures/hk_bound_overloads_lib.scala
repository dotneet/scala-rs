package hkl

object Ins {
  def forAll[E, C[_], R](xs: C[E])(fun: E => R)(implicit ev: C[E] <:< Iterable[E]): String = "coll " + ev(xs).map(fun).toList
  def forAll[K, V, MAP[k, v] <: scala.collection.GenMap[k, v], R](xs: MAP[K, V])(fun: ((K, V)) => R): String = "map " + xs.map(fun).toList
  def forAll[K, V, JMAP[k, v] <: java.util.Map[k, v], R](xs: JMAP[K, V])(fun: java.util.Map.Entry[K, V] => R): String = "jmap " + xs.size
  def forAll[R](xs: String)(fun: Char => R): String = "string " + xs.toList.map(fun)
}
