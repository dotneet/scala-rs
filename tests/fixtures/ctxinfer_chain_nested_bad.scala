final class Chain[A, B](val value: B) {
  def flatMap[C](f: B => C): Chain[A, C] = new Chain[A, C](f(value))
  def flatMap[C](other: Chain[B, C]): Chain[A, C] = new Chain[A, C](other.value)
}
object Main {
  final case class Request(id: Int)
  final case class Body(text: String)
  val request = new Chain[Request, Body](Body("body"))
  val incompatible = new Chain[String, String]("body")
  val result = request.flatMap(incompatible.flatMap(_.length))
}
