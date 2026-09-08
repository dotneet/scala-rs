// The half that says reducing a constructor alias is a *rule*, not a licence.
//
// `type G[X] = List[X]` reduces exactly the way `type AnyConstr[X] = Any`
// does; the difference is that its body still mentions the parameter, so the
// comparison it licenses has something to refuse. All three lines below would
// compile if the alias were read as "any one-parameter constructor", and real
// scalac 2.13.16 rejects all three.
object Bad {
  type AnyConstr[X] = Any
  type G[X] = List[X]

  trait Ops[+A, +CC[_], +C]

  def wantsG(x: Ops[Int, G, _]): Int = 1
}

import Bad._

// `Option` is a one-parameter constructor and `G` reduces to `List[x]`, so
// the bodies decide it and `Option[x]` is not a `List[x]`.
class OptionOps extends Ops[Int, Option, String]

// An *abstract* constructor is not `List` either: nothing is known about
// `CC[x]` beyond its kind.
trait AbstractOps[+CC[_]] extends Ops[Int, CC, String] {
  def toG: Int = wantsG(this)
}

object Main {
  def optionToG(x: Ops[Int, Option, String]): Int = wantsG(x)

  // The reduction is not symmetric. `AnyConstr` conforms to nothing in
  // particular: reducing it on the *found* side says only that the argument is
  // an `Ops` of some constructor, which is not an `Ops[Int, CC, String]`.
  def anyConstrToAbstract[CC[_]](x: Ops[Int, AnyConstr, String]): Ops[Int, CC, String] = x

  def main(args: Array[String]): Unit = println(optionToG(new OptionOps))
}
