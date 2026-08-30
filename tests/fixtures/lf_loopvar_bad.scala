// The frame for a loop-carried local is its *declared* type, so a body that
// stores something unrelated into it is still a type error -- the merge must
// not be used to quietly widen the variable to `Any`.
object Main {
  def main(args: Array[String]): Unit = {
    var c: Option[Int] = Some(1)
    while (c.isDefined) { c = "not an option" }
    println(c)
  }
}
