object Main {
  def main(args: Array[String]): Unit = {
    val xs = Array(1, 2, 3)
    println(xs.lengthIs)
    println(xs.sizeIs)
    println(xs.indexOf(2, 0))
    println(xs.indexOf(9, 0))
    val buf = Array(0, 0, 0, 0)
    println(xs.copyToArray(buf))
    println(buf(0))
    println(buf(2))
    println(buf(3))
    val it = xs.iterator
    println(it.next())
    println(it.next())
  }
}
