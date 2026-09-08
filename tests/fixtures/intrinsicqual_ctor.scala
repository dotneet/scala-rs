// Fixture for the `agent/intrinsicqual` slice, second defect: a `private` or
// `protected` constructor is access-checked.
//
// This is the **legal** side, and it is the one that matters: the check is a
// rejection rule, so what it must not do is refuse any of these. Every shape
// here compiles under real scalac 2.13.16 and under an unmodified build of
// the branch point, and prints the same thing.
//
// The rejections live in `intrinsicqual_ctor_bad.scala` and in
// `crates/cli/tests/accessctor.rs`.

package iqpkg

class Priv private (val n: Int) {
  // Inside the class itself.
  def twin: Priv = new Priv(n)
}
object Priv {
  // The companion shares the class's private access.
  def make(n: Int): Priv = new Priv(n)
}

class Prot protected (val s: String) {
  def again: Prot = new Prot(s)
}
object Prot {
  def make(s: String): Prot = new Prot(s)
}
// A subclass calls its parent's protected constructor -- as a *parent*
// constructor, which is not a `new` and is legal. `new Prot("x")` written in
// this class's body is not, and scalac says so; see the `_bad` fixture.
class Sub extends Prot("sub")

// `private[iqpkg]` is accessible from anywhere in the package.
class Qual private[iqpkg] (val q: Int)
object Elsewhere {
  def build: Qual = new Qual(9)
}

// A class whose *other* constructor is the accessible one: nsc drops the
// inaccessible alternatives before resolving, so this is not an access error.
class Mixed(val v: String) {
  private def this(len: Int) = this("len" + len.toString)
}
object MixedUse {
  def build: Mixed = new Mixed("direct")
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Priv.make(3).twin.n)
    println(Prot.make("p").again.s)
    println(new Sub().s)
    println(Elsewhere.build.q)
    println(MixedUse.build.v)
  }
}
