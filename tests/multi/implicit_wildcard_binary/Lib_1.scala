// The library half of the "a wildcard import poisons the pickle memo" test.
// Compiled separately, by **real scalac**, so the consumer sees only the class
// files and nsc's own pickle and never this source. That is the only setting
// the defect appears in: gitbucket reaches slick's conversions out of the
// published blocking-slick jar.
//
// The shape is blocking-slick's, reduced. What matters is:
//
//   * the conversions are declared on a trait *nested* inside another trait
//     (`Profile#API`), so the consumer's symbol for it starts as a `-cp` stub
//     with no members and nothing has adopted it when the import is resolved;
//   * they are reached through a **value** (`val api: API`), not an object,
//     which is what makes `import TheProfile.api._` walk the class;
//   * the wildcard also carries a name the consumer uses in a *signature*
//     (`Leafy`), so the class really is adopted a moment later -- that is the
//     `adopt_binary_class` whose members the stale memo used to replace.
//
// See `docs/gitbucket.md`, "Fixed: a wildcard import poisoned the pickle memo".
package iglib

class Node(val label: String)

class Rich[T](val self: T) {
  def described: String = "rich:" + self.toString
}

class Named[T](val self: T, val name: String) {
  def naming: String = name + "=" + self.toString
}

trait Namer[T] {
  def name: String
}

object Namer {
  implicit val intNamer: Namer[Int] = new Namer[Int] { def name: String = "int" }
}

class Plain[T](val self: T) {
  def plainly: String = "plain:" + self.toString
}

trait Profile {
  trait Tables {
    type Leafy = Node
  }
  trait API extends Tables {
    implicit def toRich[T](x: T): Rich[T] = new Rich[T](x)
    // A second clause the *class file* cannot describe: nothing in bytecode
    // records that a parameter list is implicit. If the consumer ends up with
    // the class file reader's description rather than the pickled one, this
    // reads as an ordinary two-clause method and no conversion can fire.
    implicit def toNamed[T](x: T)(implicit ev: Namer[T]): Named[T] =
      new Named[T](x, ev.name)
    // Deliberately *not* implicit: the guard must supply the pickled
    // signatures, not mark everything the class declares implicit.
    def toPlain[T](x: T): Plain[T] = new Plain[T](x)
  }
  val api: API
}

object TheProfile extends Profile {
  val api: API = new API {}
}
