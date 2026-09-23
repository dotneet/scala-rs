object Main {
  def pairs(values: Seq[IterableOnce[(Int, String)] with Equals]): Map[Int, String] =
    values.flatten.toMap

  def main(args: Array[String]): Unit = {
    val values = Seq(Some(1 -> "one"), Seq(2 -> "two"))
      .asInstanceOf[Seq[IterableOnce[(Int, String)] with Equals]]
    println(pairs(values).toSeq.sortBy(_._1).mkString(","))
  }
}
