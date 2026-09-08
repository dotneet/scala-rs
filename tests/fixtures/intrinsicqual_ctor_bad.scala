// The rejection side of the constructor access check.
//
// Every line marked below is one real scalac 2.13.16 rejects on this same
// file, at the same line, with the same sentence. Before this slice all four
// compiled: `new C(…)` is not a member selection, so `type_select`'s access
// check never saw the `<init>`.

package iqbad

class Priv private (val n: Int)
class Prot protected (val s: String)

object Outsider {
  // constructor Priv in class Priv cannot be accessed in object Outsider
  //   from object Outsider in package iqbad
  val a = new Priv(1)
  // constructor Prot in class Prot cannot be accessed in object Outsider
  //   from object Outsider in package iqbad
  val b = new Prot("x")
}

// A subclass may write `extends Prot(…)` -- that is a parent constructor, not
// a `new` -- but not `new Prot(…)`: nsc weighs the protected access against
// the prefix type, and `Prot` does not conform to `Sub`.
class Sub extends Prot("ok") {
  val c = new Prot("no")
}
