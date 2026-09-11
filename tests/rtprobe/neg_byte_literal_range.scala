// scalac: type mismatch; found Int(128), required Byte.
object Main {
  def main(args: Array[String]): Unit = { val b: Byte = 128; println(b) }
}
