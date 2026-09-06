object Main extends App {
  def union[A](x: Set[A], y: Set[A]): Set[A] = x ++ y
  val options = Set() ++ Option("sql")
  println(options.toList.sorted.mkString(","))
  println(union(Set(1, 2), Set(2, 3)).toList.sorted.mkString(","))
  println((scala.collection.immutable.SortedSet(1, 2) ++ scala.collection.immutable.SortedSet(2, 3)).mkString(","))
  val widened: Set[Any] = scala.collection.immutable.SortedSet(1) ++ List("x")
  println(widened.contains(1))
  println(widened.contains("x"))
  val mapped: scala.collection.immutable.SortedMap[Int, Any] =
    scala.collection.immutable.SortedMap(2 -> "b") ++ List(1 -> 42)
  println(mapped.keys.mkString(","))
  println(mapped(1))
  val stringKeys: scala.collection.immutable.SortedMap[String, Int] =
    scala.collection.immutable.SortedMap("b" -> 2) ++ Map("a" -> 1)
  println(stringKeys.keys.mkString(","))
  println(stringKeys("a"))
}
