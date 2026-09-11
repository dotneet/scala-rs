// scalac: type mismatch; found String("x"), required (?, ?). A view of the
// `Map` module must not make another `apply` take these arguments tupled.
object Main {
  def main(args: Array[String]): Unit = println(Map(1 -> 2, "x"))
}
