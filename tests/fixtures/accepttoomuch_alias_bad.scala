// `agent/accepttoomuch`: a written type annotation naming nothing. Every one
// of these compiled without a word before the slice; real scalac 2.13.16
// reports exactly four `not found: type Zork`, one per line.
object Bad {
  def viaParam(x: Zork): Int = 3
  def viaResult(x: Int): Zork = null
  val viaField: Zork = null
  def viaArg(x: List[Zork]): Int = 3
}
