import scala.util.chaining.scalaUtilChainingOps

class Counter {
  implicit val scale: Int = 1
  implicit val suffix: String = "items"

  private def count(source: Option[Map[Int, Seq[Int]]], ids: Seq[Int]): Int = 0
  def count(source: Map[Int, Seq[Int]])(implicit scale: Int, suffix: String): Option[Int] =
    Some(source.values.map(_.size).sum * scale)
  def count(source: Seq[(Int, Int)])(implicit scale: Int, suffix: String): Option[Int] =
    source.groupMap(_._1)(_._2).pipe(count)

  def run: Int = count(Seq((1, 2), (1, 3))).get
}

object Main {
  def main(args: Array[String]): Unit = println(new Counter().run)
}
