// `-no-specialization` is nsc's own flag: "Ignore @specialize annotations."
// This subset has no specialisation phase, so `@specialized` is a diagnostic
// without the flag (the emitted class would silently lack the `$mc*$sp`
// members callers link against) and dropped with it, exactly as nsc does.
// The 2.13 standard library writes both spellings; see docs/scala-library.md.
import scala.{specialized => sp}
import scala.annotation.unspecialized

class Cell[@specialized(Int, Long) A](a: A) {
  @unspecialized def show: String = "" + a
}

object Main {
  def id[@sp(Int, Long) T](t: T): T = t

  def main(args: Array[String]): Unit = {
    println(new Cell(1).show)
    println(new Cell("s").show)
    println(id(2))
  }
}
