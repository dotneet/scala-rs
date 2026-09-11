// The near misses of `gbmac_typer.scala`, which real scalac 2.13.16 rejects
// and scala-rs must reject too: seeing an inherited alias through the
// subclass and reading a jar case class's `copy` from its pickle must not
// make wrong programs typecheck.
import slick.util.DumpInfo

trait Domain { type Reader }
trait IntDomain extends Domain { type Reader = Int }
abstract class Conv[M <: Domain, T] {
  type Reader = M#Reader
  def read(pr: Reader): T
}
// `Reader` is `Int` here, not `String`.
class WrongResult extends Conv[IntDomain, String] {
  def read(pr: Reader): String = pr
}
// With `M` still abstract, `Reader` is `M2#Reader`, which is not `Int`.
class Abstract[M2 <: Domain] extends Conv[M2, String] {
  def asInt(r: Reader): Int = r
  def read(pr: Reader): String = ""
}

object Main {
  def main(args: Array[String]): Unit = {
    val d = DumpInfo("a")
    println(d.copy(nome = "b"))
    println(d.copy(name = 1))
    println(d.copy(name = "x", name = "y"))
  }
}
