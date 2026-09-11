// scala-rs rejects: scala.collection.mutable.StringBuilder implements
// java.lang.CharSequence, so it satisfies an upper bound of CharSequence.
object Main {
  def lenOf[S <: CharSequence](s: S): Int = s.length
  def main(args: Array[String]): Unit = {
    println(lenOf(new StringBuilder("xy")))
    val cs: CharSequence = new StringBuilder("abc")
    println(cs.length + " " + cs.charAt(1))
  }
}
