class Acc(var n: Int) {
  def +=(k: Int): Acc = { n += k; this }
}
object Main {
  def main(args: Array[String]): Unit = {
    var x = 40
    x += 1
    println(x)
    val a = new Acc(1)
    a += 1
    println(a.n)
  }
}
