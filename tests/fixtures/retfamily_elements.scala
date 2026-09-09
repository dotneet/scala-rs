import scala.collection.immutable.Queue
object Main {
  def seq(xs: Seq[Int]): Seq[String] = xs.map(_.toString)
  def queue(xs: Queue[Int]): Queue[String] = xs.map(_.toString)
  def dict(xs: Map[String, Int]): Map[String, String] = xs.map { case (k, v) => (k, v.toString) }
  def lazyMap[A, B](xs: LazyList[A], f: A => B): LazyList[B] = xs.map(f)
  def main(args: Array[String]): Unit = {
    println(seq(List(1, 2)).mkString(","))
    println(queue(Queue(3, 4)).mkString(","))
    println(dict(Map("a" -> 5))("a"))
    println(lazyMap(LazyList(6, 7), (i: Int) => i.toString).mkString(","))
  }
}
