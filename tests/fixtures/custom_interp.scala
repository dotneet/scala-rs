object Main {
  implicit class Q(sc: StringContext) {
    def q(args: Any*): String = "q:ok"
  }
  def main(args: Array[String]): Unit = {
    val x = "X"
    println(q"a$x")
  }
}
