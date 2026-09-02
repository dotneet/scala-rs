// Reading `A#B`'s members through `A` must not turn into "anything goes":
// each of these is the error nsc 2.13.16 reports.
class Sess(val name: String) {
  def database: Int = 7
}
class Other(val tag: Int)

trait Base {
  type S >: Null <: AnyRef
  trait Ctx {
    def session: S
  }
}

trait Sub extends Base { type S = Sess }
trait Alt extends Base { type S = Other }

object Bad {
  // 1. the prefix settles nothing: `S` is still abstract.
  def a(ctx: Base#Ctx): Int = ctx.session.database
  // 2. the prefix settles `S = Other`, which has no `database`.
  def b(ctx: Alt#Ctx): Int = ctx.session.database
  // 3. the settled type is real, but the member is not.
  def c(ctx: Sub#Ctx): Int = ctx.session.nosuch
}
