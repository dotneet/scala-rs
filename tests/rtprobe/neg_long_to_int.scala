// scalac: type mismatch; found Long(1L), required Int.
object Main {
  def main(args: Array[String]): Unit = { val i: Int = 1L; println(i) }
}
