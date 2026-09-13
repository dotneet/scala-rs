// A macro whose source signature has an explicit clause followed by an
// implicit clause. The typer fills `clue` at the call site, so the expansion
// request must still carry two argument clauses to the implementation.
import scala.language.experimental.macros

object CurriedImplicit {
  def identity(value: Int)(implicit clue: String): Int =
    macro CurriedImplicitImpl.identityImpl
}

object Main {
  implicit val clue: String = "ok"

  def main(args: Array[String]): Unit = {
    println(CurriedImplicit.identity(7))
  }
}
