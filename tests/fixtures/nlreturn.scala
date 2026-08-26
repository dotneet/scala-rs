object Main {
  def find(xs: List[Int]): Int = {
    xs.foreach((x: Int) => { if (x > 0) return x })
    0
  }
  def nested: Int = {
    def inner: Int = { return 1 }
    inner
  }
  def main(args: Array[String]): Unit = {
    println(find(1 :: 2 :: Nil))
    println(find((-1) :: 3 :: Nil))
    println(find((-1) :: (-2) :: Nil))
    println(nested)
  }
}
