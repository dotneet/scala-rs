// Compiled by *scalac* first, so scala-rs reads it from class files and
// pickles, as it reads twirl and scalatra.
package gblib

// twirl's `BaseScalaTemplate` is a case class; its class file carries the
// companion's `public static Base apply(F)` forwarder.
case class Base[F](format: F)

// scalatra's `ScalatraBase` declares `requestPath(implicit request)`
// abstractly; `ScalatraFilter` implements it and overloads it.
trait SB { def rp(implicit r: Int): String }
trait SF extends SB {
  def rp(implicit r: Int): String = "f" + r
  def rp(u: String, i: Int): String = u + i
}
trait FM extends SB { def x = 1 }
