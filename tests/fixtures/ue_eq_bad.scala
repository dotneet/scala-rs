// Materialising `BoxedUnit` at a comparison operand must not make the typer
// any more permissive: `Unit` is still `Unit`, and it is still not an
// `AnyRef`. Real scalac 2.13.16 rejects all three of these too.
object Main {
  def main(args: Array[String]): Unit = {
    val s: String = ()
    println(() eq ())
    println(().length)
  }
}
