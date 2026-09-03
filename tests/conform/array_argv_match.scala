// コマンド行を Array のパターンで分岐する。
object Main {
  def run(argv: Array[String]): String = argv match {
    case Array()                 => "usage"
    case Array("help")           => "help"
    case Array("add", a, b)      => (a.toInt + b.toInt).toString
    case Array("sum", rest @ _*) => rest.map(_.toInt).sum.toString
    case Array(cmd, _*)          => s"unknown:$cmd"
  }

  def pairUp(xs: Array[Int]): Array[(Int, Int)] =
    xs.grouped(2).collect { case Array(a, b) => (a, b) }.toArray

  def main(args: Array[String]): Unit = {
    println(run(Array()))
    println(run(Array("help")))
    println(run(Array("add", "2", "3")))
    println(run(Array("sum", "1", "2", "3")))
    println(run(Array("nope", "x")))
    println(pairUp(Array(1, 2, 3, 4)).map { case (a, b) => s"$a-$b" }.mkString(","))
    val Array(x, y) = Array(10, 20)
    println(x + y)
  }
}
