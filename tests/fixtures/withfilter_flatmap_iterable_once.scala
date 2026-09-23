object Main {
  def values(n: Int): Seq[(String, Int)] = Seq(("value", n))

  def gather(children: IndexedSeq[Int]): Seq[(String, Int)] =
    for {
      (child, index) <- children.toSeq.zipWithIndex
      (name, value) <- values(child)
    } yield (name, value + index)

  def main(args: Array[String]): Unit =
    println(gather(IndexedSeq(1, 2)).map(_._2).mkString(","))
}
