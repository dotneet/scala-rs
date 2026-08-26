object Main {
  def main(args: Array[String]): Unit = {
    val it = (1 :: 2 :: Nil).iterator
    println(it.hasNext)
    println(it.next)
    println(it.next)
    println(it.hasNext)
  }
}
