import scala.collection.immutable.{IntMap, LongMap}
object Bad {
  val im: IntMap[Int] = IntMap(1 -> 2).map { case (k, v) => (k.toString, v) }
  val lm: LongMap[Int] = LongMap(1L -> 2).map { case (k, v) => (k.toString, v) }
}
