// `map`'s declared result type is checked, not invented: the collection
// shortcut used to rewrite it to `Act[<element>]` and hide the mismatch.
trait NoStream
trait Effect
trait Act[+R, +S <: NoStream, -E <: Effect] {
  def value: R
  def map[R2](f: R => R2): Act[R2, NoStream, E] = sys.error("boom")
}
object M {
  def bad(a: Act[Int, NoStream, Effect]): Act[String, NoStream, Effect] =
    a.map((x: Int) => x)
}
