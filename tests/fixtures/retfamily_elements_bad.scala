import scala.collection.immutable.Queue
object Bad {
  val q: Queue[String] = Queue(1).map(_ + 1)
  val s: Seq[String] = List(1).map(_ + 1)
}
