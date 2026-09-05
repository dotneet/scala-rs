// A self type named through an `import` whose prefix is another template's
// `val` resolves, whichever order the two templates are written in.
//
// The second half of `docs/gitbucket.md`'s entry 1. gitbucket writes
// `trait TemplateComponent { self: Profile => import profile.api._;
// trait BasicTemplate { self: Table[?] => … } }`, with `Profile` in
// `model/Profile.scala` and `TemplateComponent` in `model/BasicTemplate.scala`
// -- which sorts *first* on the command line, so the import prefix cannot
// resolve while `TemplateComponent`'s signatures are being built. The nested
// self type was therefore bound with `Table` out of scope, and
// `not found: type Table` was permanent: a class header is typed by both
// passes, but a diagnostic is never retracted, so the body pass's success was
// invisible. Everything the template should have offered went with it.
//
// `Provider` deliberately comes *after* its user here, which is the whole
// point; moving it above `Component` made the same file compile.
//
// Real scalac 2.13.16 compiles this and prints the same output.

class MyTable(val n: String) {
  def col(x: String): String = n + "." + x
}

trait Component { self: Provider =>
  import api._

  trait BasicTemplate { self: Tab =>
    val userName = col("USER_NAME")
  }
}

trait Provider {
  val api: Api
}

class Api {
  type Tab = MyTable
}

object Holder extends Component with Provider {
  val api = new Api
  class Row(m: String) extends MyTable(m) with BasicTemplate
}

object Main {
  def main(args: Array[String]): Unit = println(new Holder.Row("rows").userName)
}
