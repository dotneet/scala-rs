// Call sites for `rb_impl.scala`, compiled against its class files the way
// nsc requires -- the implementation has to come from an earlier run.
//
// Every expansion here arrives as an `Expr` built by a `TreeCreator` that
// `reify` synthesised. Real scalac 2.13.16 compiles the same two files
// against each other and the two programs must print the same thing: a
// creator that resolved `RbHelper` in the wrong universe, or spliced an
// argument's tree without rebasing it, would still compile.
import scala.language.experimental.macros

object RbUse {
  def fortyTwo: Int = macro RbImpl.fortyTwo
  def hello: String = macro RbImpl.hello
  def yes: Boolean = macro RbImpl.yes
  def big: Long = macro RbImpl.big
  def helper: Int = macro RbImpl.helper
  def twice(x: Int): Int = macro RbImpl.twice
  def sum(a: Int, b: Int): Int = macro RbImpl.sum
  def join(a: String): String = macro RbImpl.join
  def flipped(b: Boolean): Boolean = macro RbImpl.flipped
  def qualified: Int = macro RbImpl.qualified
}

object Main {
  def main(args: Array[String]): Unit = {
    println(RbUse.fortyTwo)
    println(RbUse.hello)
    println(RbUse.yes)
    println(RbUse.big)
    println(RbUse.helper)
    println(RbUse.twice(21))
    println(RbUse.sum(20, 22))
    println(RbUse.join("head"))
    println(RbUse.flipped(true))
    println(RbUse.qualified)
    // The splices are evaluated where they stand, so a reified body that
    // dropped or duplicated one shows up as a different count.
    var n = 0
    def bump(): Int = { n += 1; n }
    println(RbUse.sum(bump(), bump()))
    println(n)
  }
}
