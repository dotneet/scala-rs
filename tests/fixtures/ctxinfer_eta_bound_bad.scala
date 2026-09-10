object Main {
  def convert[A <: Number](a: A)(implicit p: String): String = p + a
  implicit val p: String = "n="
  val f: String => String = convert
}

