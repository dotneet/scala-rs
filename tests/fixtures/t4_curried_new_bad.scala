// Flattening a curried `new` must not make it accept anything: a third
// parameter list, a missing one, and a wrong argument type in the second one
// are all still errors.

object Main {
  trait TT[T]
  final case class Lit(name: String)(val buildType: Int)
  final class Ev[B](val s: String)(implicit val b: TT[B])

  def tooManyLists = new Lit("a")(1)(2)
  def wrongSecondList = new Lit("a")("b")
  def missingEvidence[B] = new Ev[B]("s")

  def main(args: Array[String]): Unit = {
    println(tooManyLists)
    println(wrongSecondList)
    println(missingEvidence[Int])
  }
}
