object Main {
  // A two-parameter literal is not a `Function1`; nsc reports a type
  // mismatch, and nothing is quietly lowered to an `invokedynamic` whose
  // call site would not verify.
  val wrong: Int => Int = (a: Int, b: Int) => a + b
  def main(args: Array[String]): Unit = println(wrong(1))
}
