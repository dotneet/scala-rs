// A self type that names a class read from a class file offers its members
// unqualified, exactly as a parent does.
//
// `docs/gitbucket.md`'s "what would remove the most next", entry 1. gitbucket
// writes every slick table mix-in as
// `trait BasicTemplate { self: Table[?] => val userName = column[String](…) }`
// and `column` was `not found: value column`. The wildcard is not what breaks
// it: `Table[String]` failed the same way, and a `Table` written in source
// worked with either. What decides it is where `Table` comes from. A `-cp`
// class's members are completed one name at a time, on demand, so
// `bind_self_type` -- which copies the self type's member list into the
// template scope -- copied an empty list. A qualified `this.column` resolved
// all along, because `type_select` completes on demand.
//
// Compiled against `ws_selftype_lib.scala`'s class files. Real scalac 2.13.16
// compiles both halves and prints the same output.
import wsl._

// The shape gitbucket writes, wildcard and all.
trait BasicTemplate { self: Table[?] =>
  val userName = column[String]("USER_NAME")
  def described: String = describe
}

// A named self type, which failed for the same reason.
trait NamedTemplate { self: Table[String] =>
  val repositoryName = column[String]("REPOSITORY_NAME")
}

// A compound self type: every part offers its members.
trait BothTemplate { self: Table[?] with Tagged =>
  def both: String = column[String]("BOTH") + "/" + tag
}

// An unqualified name written in a *nested* template still reaches the
// enclosing template's self type.
trait Outer { self: Table[?] =>
  trait Inner {
    def fromOuter: String = describe
  }
}

class Users(n: String) extends Table[String](n) with BasicTemplate with NamedTemplate

class Both(n: String) extends Table[String](n) with Tagged with BothTemplate

class Nest(n: String) extends Table[String](n) with Outer {
  object In extends Inner
}

object Main {
  def main(args: Array[String]): Unit = {
    val u = new Users("users")
    println(u.userName)
    println(u.repositoryName)
    println(u.described)
    println(new Both("both").both)
    println(new Nest("nest").In.fromOuter)
  }
}
