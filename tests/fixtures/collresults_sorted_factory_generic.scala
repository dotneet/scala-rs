object Main {
  def zip[A](xs: scala.collection.immutable.SortedSet[A])
      (implicit ord: Ordering[A]): scala.collection.immutable.SortedSet[(A, Int)] =
    xs.zipWithIndex.to(scala.collection.immutable.SortedSet)

  def main(args: Array[String]): Unit =
    println(zip(scala.collection.immutable.SortedSet(3, 1, 2)).mkString(","))
}
