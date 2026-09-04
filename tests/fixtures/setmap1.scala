// agent/setmap: overload selection when building and adding to `Set`/`Map`, and
// `Array` being treated as a `Seq`/`Iterable`.
// All 8 slick cases collected into one file (measurement cost: 1.8 s per real scalac run).
import scala.annotation.unchecked.uncheckedVariance
import scala.collection.immutable.{HashMap, HashSet}

// slick ConstArray.toSet: `immutable.HashSet[T @uncheckedVariance]`.
final class CArr[+T](val xs: List[T]) {
  def toSet: HashSet[T @uncheckedVariance] = HashSet.from(xs)
}

sealed trait ColOpt
case class SqlType(name: String) extends ColOpt
case object AutoInc extends ColOpt
case object PrimaryKey extends ColOpt

object Main {
  // 1) Array goes through as Seq / IndexedSeq / Iterable (nsc inserts
  //    copyArrayToImmutableIndexedSeq / genericWrapArray).
  def asSeq(a: Array[Any]): Seq[Any] = a
  def asSeqInt(a: Array[Int]): Seq[Int] = a
  def asIndexed(a: Array[String]): IndexedSeq[String] = a
  def asIterable(a: Array[Any]): Iterable[Any] = a

  // 2) Array also reaches an IndexedSeq parameter on the overload-resolution side
  //    (slick ResultConverter.scala:58 `TupleSupport.buildTuple(a)`).
  def buildTuple(a: IndexedSeq[Any]): String = a.mkString("<", ",", ">")
  def buildTuple(a: String): String = a

  // 3) `Set() ++ Option ++ Option` (slick JdbcModelBuilder.scala:280).
  def options(dbType: Option[String], autoInc: Boolean, pk: Boolean): Set[ColOpt] =
    Set() ++
      dbType.map(s => SqlType(s)) ++
      (if (autoInc) Some(AutoInc) else None) ++
      (if (pk) Some(PrimaryKey) else None)

  // 4) Adding something of a different element type falls to `++[B >: A]`.
  def widen(s: Set[String], o: Option[Int]): Set[Any] = s ++ o

  // 5) `Map() ++ Array[(K, V)]` (slick JdbcTypesComponent.scala:526).
  def typeNames(a: Array[(Int, String)]): Map[Int, String] = Map() ++ a

  // 6) contains / apply / get on `collection.Map`
  //    (slick ExpandTables.scala:24-25).
  def expand(expansions: collection.Map[String, (String, Int)], k: String): String =
    if (expansions contains k) {
      val (sym, n) = expansions(k)
      s"$sym$n"
    } else expansions.get(k).map(_._1).getOrElse("-")

  // 7) Disambiguating `HashSet#map`, and `_._1` on an `@uncheckedVariance` element
  //    (slick PruneProjections.scala:14).
  def unreferenced(all: CArr[String], refs: CArr[(String, Int)]): HashSet[String] =
    all.toSet -- refs.toSet.map(_._1)

  // 8) `HashMap + (k -> v)` (slick QueryCompiler.scala:220).
  def put[S](state: HashMap[String, Any], k: String, v: S): HashMap[String, Any] =
    state + (k -> v)

  def main(args: Array[String]): Unit = {
    println(asSeq(Array[Any](1, "a")).mkString(","))
    println(asSeqInt(Array[Int](3, 1, 2)).sum)
    println(asIndexed(Array[String]("x", "y")).mkString("-"))
    println(asIterable(Array[Any](true, 2)).size)

    println(buildTuple(Array[Any](1, 2, 3)))
    println(buildTuple("plain"))

    println(options(Some("VARCHAR"), true, false).toList.map(_.toString).sorted.mkString(","))
    println(options(None, false, true).toList.map(_.toString).sorted.mkString(","))

    println(widen(Set("a"), Some(1)).toList.map(_.toString).sorted.mkString(","))

    // An `Array[(Int, String)](…)` literal hits a separate bug (it builds an
    // `Object[]` and checkcasts it to `[Lscala/Tuple2;`), so build it by element store.
    val pairs = new Array[(Int, String)](2)
    pairs(0) = 1 -> "one"
    pairs(1) = 2 -> "two"
    println(typeNames(pairs).toList.sortBy(_._1).mkString(","))

    val m: collection.Map[String, (String, Int)] = Map("k" -> (("s", 7)))
    println(expand(m, "k"))
    println(expand(m, "missing"))

    println(unreferenced(new CArr(List("a", "b", "c")), new CArr(List(("b", 1)))).toList.sorted.mkString(","))

    println(put(HashMap[String, Any]("a" -> 1), "b", "two").toList.sortBy(_._1).mkString(","))
  }
}
