// An implicit view whose parameter is by-name (scalatra's
// `booleanBlock2RouteMatcher(block: => Boolean): RouteMatcher`, behind every
// `get(cond) { ... }`) views a plain `Boolean` -- including into a repeated
// parameter -- and receives the argument unevaluated.
import scala.language.implicitConversions
trait RT { def run(): Boolean }
class Route
object Conv {
  implicit def lazyBool(block: => Boolean): RT = new RT { def run() = block }
  implicit def str(path: String): RT = new RT { def run() = path.nonEmpty }
}
import Conv._
object Main {
  var n = 0
  def flag: Boolean = { n += 1; println("eval " + n); true }
  def keep(t: RT): RT = { println("kept"); t }
  def many(ts: RT*): Seq[RT] = { println("many " + ts.size); ts }
  def get(transformers: RT*)(action: => Any): Route = {
    println("route " + transformers.map(_.run()).mkString(",") + " " + action); new Route
  }
  def main(args: Array[String]): Unit = {
    val t = keep(flag)
    println(t.run())
    println(t.run())
    val ts = many(flag, !flag)
    println(ts.map(_.run()))
    val u: RT = flag
    println(u.run())
    get(!flag, flag) { "x" }
    get("p") { "y" }
  }
}
