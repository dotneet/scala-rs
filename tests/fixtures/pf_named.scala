// Named arguments on the standard library's methods.
//
// A member `PickleSupply` installs from a `ScalaSignature` carries a parameter
// symbol per parameter; a member the hand-written prelude declares carries
// only types, and member lookup finds the hand-written one first. So every one
// of these was "unimplemented syntax: named arguments (method parameters not
// resolved)" -- or, for the 150 methods `prelude_seq::poly_in` builds with
// placeholder `x$1` names, "unknown parameter name".
//
// scalac 2.13.16 accepts every line.

object Main {
  val l = List(1, 2, 3)

  def main(args: Array[String]): Unit = {
    // One clause, one name.
    println(l.mkString(sep = "-"))
    // Every parameter named, in order and out of it: SLS 6.6.1 keeps the
    // written evaluation order, which `crate::named_eval_order` implements.
    println(l.mkString(start = "[", sep = ",", end = "]"))
    println(l.mkString(end = "]", start = "[", sep = ","))
    // A `poly_in` method, whose parameter symbol is called `x$1`.
    println(l.map(f = (x: Int) => x + 1))
    println(l.exists(p = (x: Int) => x > 2))
    println(l.take(n = 2))
    println(l.drop(n = 1))
    println(l.indexOf(elem = 2))
    // A second parameter clause.
    println(l.foldLeft(z = 0)(op = (a: Int, b: Int) => a + b))
    // On `StringOps` and on `Option`.
    println("abc".mkString(sep = ","))
    println(Option(1).getOrElse(default = 0))
    // A repeated parameter after a named one.
    println(List.fill(n = 3)(0))
  }
}
