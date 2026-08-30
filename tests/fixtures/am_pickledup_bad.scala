// Collapsing the copies of one pickled declaration must not collapse
// *alternatives*: two genuinely different signatures are still two, and an
// ambiguity between them is still an error. scalac rejects this too, with
// "ambiguous reference to overloaded definition".
object Main {
  def f(x: Int, y: Any): String = "a"
  def f(x: Any, y: Int): String = "b"

  def main(args: Array[String]): Unit = println(f(1, 2))
}
