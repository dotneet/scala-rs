// `A#B`: a member of `B` whose type is an abstract type member of `B`'s
// *enclosing* class has to be read at what `A` settles it to. slick writes
// `def run(ctx: HeapBackend#BasicActionContext) = f(ctx.session)`, where
// `session: Session` is declared on `BasicBackend` and only `HeapBackend`
// says `type Session = HeapSessionDef`.
class Sess(val name: String) {
  def database: Int = 7
}

trait Base {
  type S >: Null <: AnyRef
  trait Ctx {
    def session: S
    def label: String
  }
  trait Deep extends Ctx {
    def twice: S
  }
}

trait Sub extends Base {
  type S = Sess
}

object Runner extends Sub {
  final class MyCtx(val session: Sess, val label: String) extends Deep {
    def twice: Sess = session
  }
}

object Main {
  def db(ctx: Sub#Ctx): Int = ctx.session.database
  def nm(ctx: Sub#Ctx): String = ctx.session.name + "/" + ctx.label
  def take(s: Sess): String = s.name
  // The settled member type has to be good enough to *pass on*, not just to
  // select from: this is slick's `f(ctx.session)`.
  def viaFn(ctx: Sub#Ctx, f: Sess => String): String = f(ctx.session)
  // Inherited through a second nested trait, so the view survives `A#B` where
  // `B` gets the member from its own parent.
  def deep(d: Sub#Deep): String = take(d.twice)
  // A projection and the same class reached through the alias are one type.
  def alias(s: Sub#S): String = take(s)

  def main(args: Array[String]): Unit = {
    val c = new Runner.MyCtx(new Sess("s1"), "L")
    println(db(c))
    println(nm(c))
    println(viaFn(c, take))
    println(deep(c))
    println(alias(c.session))
  }
}
