// Three answers the compiler used to get quietly wrong outside pattern
// matching:
//
//   * a `case class` companion prints as the class's name -- it used to
//     inherit `AbstractFunctionN.toString` and print `<function0>`;
//   * a `var` constructor parameter is a *field*, so assigning to it in the
//     class body has to be visible to the class's methods -- the assignment
//     went to the constructor's local and the field kept the argument;
//   * `lub(Unit, A)` for an abstract `A` is `Any`, not `Unit`. Closing `A` to
//     its lower bound made the method return `void` and every case's value
//     was discarded.

case class Zero()
case class One(a: Int)

class VarParam(private[this] var c: String) {
  c = "good"
  val seen = c
  def f = c
}

class PublicVarParam(var c: String) {
  c = "good"
  def f = c
}

object Main {
  def unitOrElement(h: Any) = h match {
    case 5          => ()
    case List(from) => from
    case x          => throw new MatchError(x)
  }

  def main(args: Array[String]): Unit = {
    println(Zero)
    println(One)
    println(Zero())
    val v = new VarParam("bad")
    println(v.seen + " " + v.f)
    println(new PublicVarParam("bad").f)
    println(unitOrElement(5))
    println(unitOrElement(List(6)))
  }
}
