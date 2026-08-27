object Main {
  def main(args: Array[String]): Unit = {
    println(IndexedSeq(1, 2)(1))
    val q = scala.collection.immutable.Queue(1, 2).enqueue(3)
    val d = q.dequeue
    println(d._1)
    println(d._2.dequeue._1)
  }
}
