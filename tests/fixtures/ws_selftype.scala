// The runnable half: a self type still narrows `this`, and the members it
// offers are called on `this` at run time.
//
// `Table` here is top-level, which is the shape that always worked; the point
// of this fixture is the *code* the templates compile to -- the cast a
// self-typed trait's method needs, and the two spellings of the self type
// (`Table[?]` and `Table[String]`) side by side with a compound one.
// `ws_selfstub.scala` is where the typing root is pinned.
//
// Compiled against `ws_selftype_lib.scala`'s class files. Real scalac 2.13.16
// compiles both halves and prints the same output.
import wsl._

// The shape gitbucket writes, wildcard and all.
trait BasicTemplate { self: Table[?] =>
  val userName = column[String]("USER_NAME")
  def described: String = describe
}

// A named self type.
trait NamedTemplate { self: Table[String] =>
  val repositoryName = column[String]("REPOSITORY_NAME")
}

// A compound self type: every part offers its members.
trait BothTemplate { self: Table[?] with Tagged =>
  def both: String = column[String]("BOTH") + "/" + tag
}

class Users(n: String) extends Table[String](n) with BasicTemplate with NamedTemplate

class Both(n: String) extends Table[String](n) with Tagged with BothTemplate

object Main {
  def main(args: Array[String]): Unit = {
    val u = new Users("users")
    println(u.userName)
    println(u.repositoryName)
    println(u.described)
    println(new Both("both").both)
  }
}
