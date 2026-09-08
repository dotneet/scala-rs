// The competing definitions for the SLS 2 precedence fixture. Kept in a unit
// of its own so that `Main_1.scala` can be the "same compilation unit" for
// some of the names and not for others.
package lib

/** What a wildcard import (`import lib.Wild._`, precedence 3) offers. */
object Wild {
  val name: String = "wildcard"
  val local: String = "wildcard"
  val member: String = "wildcard"
  object Shared { def who: String = "wildcard" }
  object Sibling { def who: String = "wildcard" }
  class T { def who: String = "wildcard" }
}

/** What an explicit import (`import lib.Exp.name`, precedence 2) names. */
object Exp {
  val name: String = "explicit"
  val other: String = "explicit-renamed"
  class T { def who: String = "explicit" }
}
