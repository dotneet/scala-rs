import pureconfig._
import pureconfig.generic.auto._

case class Probe(value: String)

object Main {
  def main(args: Array[String]): Unit =
    println(ConfigSource.string("value = probe").load[Probe])
}
