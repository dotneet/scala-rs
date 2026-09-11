// The companion inherits a concrete `apply(String)` from a binary trait, so
// the synthetic case `apply` is not generated and `Parsed("")` validates
// (run/t10261).
object Parsed extends Companion[Parsed] {
  def parse(v: String) = if (v.nonEmpty) Some(new Parsed(v + "!")) else None
}
case class Parsed(value: String)

object Main {
  def main(args: Array[String]): Unit = {
    println(Parsed("a").value)
    println(scala.util.Try(Parsed("")).isFailure)
  }
}
