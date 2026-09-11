// scalac: type mismatch; found String("x"), required (?, ?). No view of the
// `Map` module may turn this into another `apply` (scala-rs used to reach
// `BuildFrom.toBuildFrom(Map).apply` with the arguments tupled).
object Main {
  def main(args: Array[String]): Unit = println(Map(1 -> 2, "x"))
}
