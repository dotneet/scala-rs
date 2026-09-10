object Main {
  def convert(a: String)(implicit n: Int): String = a + n
  implicit val n: Int = 3
  val f: Int => String = convert
}

