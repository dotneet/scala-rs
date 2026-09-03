// agent/final3 negative cases; real scalac 2.13.16 rejects both.
class NdB
final case class ComprB[+Fetch <: Option[NdB]](tag: String) extends NdB

object Final3Bad {
  def needsSome(o: Option[ComprB[Some[NdB]]]): String = o.toString

  // A wildcard argument is bounded by its parameter's declared bound
  // (`Option[NdB]`), not vacuous: `ComprB[_]` is no `ComprB[Some[NdB]]`.
  def use(n: NdB): String = n match {
    case c: ComprB[_] => needsSome(Some(c))
    case _            => "no"
  }

  // Still a real cycle: nothing to borrow a result type from.
  def loop(n: Int) = if (n <= 0) 0 else loop(n - 1)
}
