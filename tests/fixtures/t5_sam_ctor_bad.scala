// A function literal of the *wrong arity* for the SAM parameter
// (`t5_sam_ctor.scala`'s `SetParameter[Unit]` wants two parameters, this
// gives it one) is still rejected, through the same widened `arg_score`
// SAM comparison.
object Main {
  trait PositionedParameters
  trait SetParameter[-T] extends ((T, PositionedParameters) => Unit) {
    def apply(v1: T, v2: PositionedParameters): Unit
  }
  case class Builder(sql: String, setParameter: SetParameter[Unit])

  def make: Builder = Builder("x", (u: Unit) => ())

  def main(args: Array[String]): Unit = println(make)
}
