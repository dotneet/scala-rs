// With `+=` at its proper (lowest) precedence, an op-assignment to an
// immutable receiver must report nsc's `convertToAssignment` diagnostic --
// not the `any2stringadd` overload error the mis-parse `(n += i) + x` used to
// produce.
object Main {
  def main(args: Array[String]): Unit = {
    val n = 0
    val i = 1
    val x = 2
    n += i + x
    println(n)
  }
}
