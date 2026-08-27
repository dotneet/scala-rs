class Cell {
  var n: Int = 0
  def update(i: Int, v: Int): Unit = {
    n = v + i
  }
  def apply(i: Int): Int = n + i
}
object Main {
  def main(args: Array[String]): Unit = {
    val arr = new Array[Int](3)
    arr(0) = 1
    arr(1) = 2
    arr(2) = 3
    println(arr(0))
    println(arr(1))
    arr(1) = 9
    println(arr(1))
    val c = new Cell()
    c(1) = 10
    println(c.n)
    println(c(2))
  }
}
