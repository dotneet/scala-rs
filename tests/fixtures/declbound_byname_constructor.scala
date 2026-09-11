class DelayedInt(value: => Int) {
  def read: Int = value
  lazy val cached: Int = value
}
class DelayedString(value: => String) {
  def read: String = value
  lazy val cached: String = value
}
class StrictParent(val stored: String)
class Forwarded(text: => String) extends StrictParent(text) {
  def read: String = text
}
object Main {
  def main(args: Array[String]): Unit = {
    var calls = 0
    val ints = new DelayedInt({ calls += 1; calls })
    println(calls)
    println(ints.read)
    println(ints.read)
    println(ints.cached)
    println(ints.cached)
    println(calls)
    val strings = new DelayedString({ calls += 1; "s" + calls })
    println(strings.read)
    println(strings.cached)
    println(strings.cached)
    val forwarded = new Forwarded({ calls += 1; "f" + calls })
    println(forwarded.stored)
    println(forwarded.read)
    println(calls)
  }
}
