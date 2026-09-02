// An unknown parameter name is still an error once qualified named-argument
// resolution works (`t5_named_qual.scala`): "unknown parameter name: c" is
// reported through the same `place_named_args` path a bare-identifier call
// already used, now reachable for a qualified one too.

package t5npb {
  final case class Bar(a: Int, b: String)
}

object Main {
  def make = t5npb.Bar(c = 1, a = 2)
  def main(args: Array[String]): Unit = println(make)
}
