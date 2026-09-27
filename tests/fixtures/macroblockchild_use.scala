object Main {
  val result: Chain = (new Chain).append[Int].append[Int].append[Int]
  def main(args: Array[String]): Unit = println(result != null)
}
