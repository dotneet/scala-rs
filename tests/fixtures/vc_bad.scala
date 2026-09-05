// Every value-class restriction crates/typer/src/valueclass.rs implements,
// one per line, in the order nsc's `validateDerivedValueClass` checks them.
// The shapes and the expected wording are `test/files/neg/valueclasses.scala`
// and its `.check`, re-run against /tmp/scala-2.13.16/bin/scalac.
//
// Line numbers matter: crates/cli/tests/valueclass.rs asserts each message at
// the line it is written on, so scalac's own positions can be compared.
package vc

trait BadTrait extends AnyVal // only classes (not traits) are allowed

class Outer {
  class BadNested(val x: Int) extends AnyVal // may not be a member of another class
  def m(): Unit = {
    class BadLocal(val x: Int) extends AnyVal // may not be a local class
    ()
  }
}

class BadNoParam extends AnyVal // needs exactly one val parameter
class BadTwoParams(val x: Int, val y: String) extends AnyVal // needs exactly one
class BadTwoClauses()(val a: Any) extends AnyVal // needs exactly one
class BadContextBound[T: Ordering](val x: T) extends AnyVal // the evidence clause is a second clause
class BadVar(var x: Int) extends AnyVal // must not be a var
class BadBare(x: Int) extends AnyVal // must be a val and not be private[this]
class BadPrivateThis(private[this] val x: Int) extends AnyVal // ditto
class BadProtectedThis(protected[this] val x: Int) extends AnyVal // must not be protected[this]

class BadField(val x: Int) extends AnyVal {
  val y = x // field definition is not allowed
}

class BadSpecialized[@specialized T, U](val x: (T, U)) extends AnyVal // may not be specialized
