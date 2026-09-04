// agent/testkit: the two roots that stopped slick-testkit at the parser and
// at `import tdb.profile.api.*`.
//
//  1. `for (case p <- xs)` -- Scala 3's spelling of a filtering generator.
//     scalac 2.13.16 accepts the `case` marker with no `-Xsource` flag at all;
//     `JdbcMapperTest.scala` uses it twice and the parse aborted the run.
//  2. a guard on a *destructuring* generator has to see what the pattern
//     binds. The guard closure was built from `pat.name()`, which is `None`
//     for `(i, s)`, so the parameter was `_` and `i` was "not found".
//  3. an import prefix of three or more segments (`d.profile.api`), and one
//     whose head is a `val` of the *same* template. Both are typed before the
//     template's signatures exist; `type_select` only retypes a qualifier that
//     is still `NoType`, so the first pass's `Error` was permanent.

object TkDefs {
  trait Api { def column(n: String): Int = n.length }
  trait Profile { type API <: Api; val api: API }
  trait P2 extends Profile { type API = Api }
  // (`type Prof <: P2` rather than `<: Profile`: overriding `profile`
  // through a *widened* abstract type member needs an erasure bridge this
  // compiler does not emit yet -- a separate gap, reported, not tested here.)
  trait DB { type Prof <: P2; val profile: Prof }
  trait DB2 extends DB { type Prof = P2 }

  object TheApi extends Api
  object TheProfile extends P2 { val api: API = TheApi }
  object TheDB extends DB2 { val profile: Prof = TheProfile }
}

import TkDefs._

abstract class TkBase { val d: DB2 }
// 3a: three-segment prefix, head inherited from the parent.
abstract class TkThreeStep extends TkBase {
  import d.profile.api._
  def viaImport: Int = column("abcd")
}
// 3b: the prefix's head is declared in this very template.
class TkSameTemplate {
  val p: P2 = TheProfile
  import p.api._
  def same: Int = column("xyz")
}

object Main {
  val xs: List[(Int, String)] = List((1, "a"), (2, "b"), (3, "c"))

  // 1: `case` on a generator.
  val a: List[String] = for (case (i, s) <- xs) yield s + i
  // 1 + 2: `case` plus a guard that reads the pattern's bindings.
  val b: List[String] = for { case (i, s) <- xs if i > 1 } yield s
  // 2: the same guard without the marker, and as a separate enumerator.
  val c: List[String] = for { (i, s) <- xs if i % 2 == 1 } yield s + i
  val d2: List[String] = for { (i, s) <- xs; if i < 3 } yield s + i

  def main(args: Array[String]): Unit = {
    println(a.mkString(","))
    println(b.mkString(","))
    println(c.mkString(","))
    println(d2.mkString(","))
    val u = new TkThreeStep { val d: DB2 = TheDB }
    println(u.viaImport)
    println(new TkSameTemplate().same)
  }
}
