package object enrich {
  implicit class Rich(n: Int) {
    def twice: Int = n * 2
  }
}
object Main {
  import enrich._
  def main(args: Array[String]): Unit = {
    println(2.twice)
  }
}
