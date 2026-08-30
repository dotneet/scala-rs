// `-=` on a receiver that has neither the operator nor an assignable `-`.
// nsc reports this as one error whose message carries a second explanatory
// line, not as two separate errors.
class Plain

object Main {
  def main(args: Array[String]): Unit = {
    val p = new Plain
    p -= 2
  }
}
