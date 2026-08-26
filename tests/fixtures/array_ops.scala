object Main {
  def main(args: Array[String]): Unit = {
    val arr = Array(1, 2, 3)
    println(arr(0))
    println(arr.length)
    arr.update(1, 9)
    println(arr(1))
    arr(2) = 8
    println(arr(2))
  }
}
