// `?` is reserved for the wildcard type, so it cannot name a type without
// backticks — scalac: "using `?` as a type name requires backticks".
object Main {
  type ?[A, B] = Map[A, B]

  def main(args: Array[String]): Unit = println(1)
}
