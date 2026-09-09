// The owner rule demotes an *inherited* conversion. Two conversions whose
// owners are unrelated still tie, and a tie is still refused.
//
// This is the guard on `agent/strarrayops`: the fix must not turn every
// ambiguous implicit view into a silent pick.
import scala.language.implicitConversions

class POps(val s: String) { def sel: String = "p" }
class QOps(val s: String) { def sel: String = "q" }

object P {
  implicit def toP(s: String): POps = new POps(s)
}
object Q {
  implicit def toQ(s: String): QOps = new QOps(s)
}

object Main {
  import P._
  import Q._
  def f: String = "x".sel
  def main(args: Array[String]): Unit = println(f)
}
