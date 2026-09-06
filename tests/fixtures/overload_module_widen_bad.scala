object Main {
  val bad: scala.collection.immutable.SortedSet[Any] =
    scala.collection.immutable.SortedSet(1) ++ List("x")
}
