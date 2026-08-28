object Main {
  implicit class B(val sc: StringContext) { def b(args: Any*): String = sc.parts.mkString("|") }
  def main(args: Array[String]): Unit = {
    println(b"\{")
    println(b"a\)b")
    println(s"tab\there")
  }
}
