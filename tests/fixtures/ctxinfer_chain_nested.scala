final class Chain[A, B](val value: B) {
  def flatMap[C](f: B => C): Chain[A, C] =
    new Chain[A, C](f(value))

  def flatMap[C](other: Chain[B, C]): Chain[A, C] =
    new Chain[A, C](other.value)

  def asScala: Chain[A, B] = this
}

object Main {
  final case class Request(id: Int)
  final case class Body(text: String)

  def main(args: Array[String]): Unit = {
    val request = new Chain[Request, Body](Body("body"))
    val existingChainBodyToString = new Chain[Body, String]("body")
    val result = request.flatMap(existingChainBodyToString.flatMap(_.length)).asScala
    val checked: Chain[Request, Int] = result
    println(checked.value)
  }
}
