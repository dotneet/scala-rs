trait TT[T] { def name: String }

object Api {
  def typed(s: String): String = s
  def typed[T: TT](n: Int): String = implicitly[TT[T]].name + n
}

object Main {
  implicit val ttInt: TT[Int] = new TT[Int] { def name = "int" }
  def main(args: Array[String]): Unit = {
    // Narrowing the overload by the explicit type argument must not make the
    // missing `TT[String]` disappear: there is no witness for it.
    println(Api.typed[String](1))
  }
}
