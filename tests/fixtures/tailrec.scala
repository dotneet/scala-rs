object Main {
  @tailrec
  def sum(n: Int, acc: Int): Int =
    if (n <= 0) acc else sum(n - 1, acc + n)
  def main(args: Array[String]): Unit = {
    println(sum(5, 0))
  }
}
