// `new C` with no argument list, where the type arguments are written: the
// constructor's parameter is the argument's type, not the declaration's type
// parameter, so nothing may be adapted from `()`. scala-rs read the declared
// `T`, took it for an open parameter and accepted the call (agent/erascg).
class Gen[T](a: T)
class Pair[A, B](a: A, b: B)
object Test {
  val one = new Gen[Int]
  val two = new Pair[Int, String]
}
