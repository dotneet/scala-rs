import scala.collection.immutable.List

object Warm {
  val values: List[Int] = List(1).lazyZip(List(2)).map((x, y) => x + y)
}
