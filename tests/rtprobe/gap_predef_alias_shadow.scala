// scala-rs rejects: `Predef.String` inside a scope that defines its own
// class String -- the alias must still mean java.lang.String.
object Main {
  object Shadows {
    class String(val raw: Predef.String) { def len = raw.length }
    def mk(s: Predef.String): Predef.String = s + "!"
  }
  def main(args: Array[String]): Unit = {
    import Shadows._
    println(new String("four").len + " " + mk("x"))
  }
}
