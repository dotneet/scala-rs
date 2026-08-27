object Main {
  def main(args: Array[String]): Unit = {
    import scala.util.chaining._
    println(1.pipe(_ + 1))
    var seen = 0
    val x = 7.tap((n: Int) => { seen = n })
    println(x)
    println(seen)
  }
}
