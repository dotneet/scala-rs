// A self type named through an `import` whose prefix is another template's
// `val` resolves, whichever order the two templates are written in.
//
// The second half of `docs/gitbucket.md`'s entry 1. gitbucket writes
// `trait TemplateComponent { self: Profile => import profile.api._;
// trait BasicTemplate { self: Table[?] => … } }`, with `Profile` in
// `model/Profile.scala` and `TemplateComponent` in `model/BasicTemplate.scala`
// -- which sorts *first*. An import prefix that is another template's `val`
// cannot resolve during that template's signature pass, so `Table` was not in
// scope when the nested self type was bound, and `not found: type Table` (and
// with it every member the template should have offered) was permanent: a
// class header is typed by both passes, but the signature pass's diagnostic
// was kept.
//
// `Provider` deliberately comes *after* its user here, which is the same
// ordering the two gitbucket files have.
import wsl._

trait Component { self: Provider =>
  import api._

  trait BasicTemplate { self: Table[?] =>
    val userName = column[String]("USER_NAME")
  }
}

trait Provider {
  val api: Api
}

class Api {
  type Table[T] = wsl.Table[T]
}

class Users(n: String) extends Table[String](n)

object Holder extends Component with Provider {
  val api = new Api
  class Row(n: String) extends wsl.Table[String](n) with BasicTemplate
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Holder.Row("rows").userName)
  }
}
