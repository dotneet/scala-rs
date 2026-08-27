object Main {
  def main(args: Array[String]): Unit = {
    import scala.util.chaining._
    println(1.pipe(_ + 1))
    val box = Array(0)
    val x = 7.tap((n: Int) => { box(0) = n })
    println(x)
    println(box(0))
  }
}
