// scalac: type mismatch; found Int, required Char (only constant Ints narrow).
object Main {
  def main(args: Array[String]): Unit = { val i = 65; val c: Char = i; println(c) }
}
