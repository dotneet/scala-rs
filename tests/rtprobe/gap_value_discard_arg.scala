// scala-rs rejects: value discarding of an Int argument passed to a Unit
// parameter.
object Main {
  var x = 0
  def sideEffect(): Int = { x += 1; x }
  def unitParam(u: Unit): String = "got " + u
  def main(args: Array[String]): Unit = println(unitParam(sideEffect()) + " " + x)
}
