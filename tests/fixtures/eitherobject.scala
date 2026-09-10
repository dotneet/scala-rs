object Main {
  def same(e: Either.type): Either.type = e
  def main(args: Array[String]): Unit = {
    val e: Either.type = same(Either)
    println(e.cond(true, 7, "no"))
    println(Either.cond(false, 8, "no"))
    println(e eq scala.util.Either)
  }
}
