// Compiled by real scalac and consumed from a class-file directory.
// Several declarations make the shallow eager scan observable: the JVM
// methods are present, but only the pickle says which ones are implicit.
package ctxreg

trait Ctx

class Out(val text: String)

object Implicits {
  // Force the enclosing object to be encountered through a nested class too.
  class Nested(val value: Int)

  implicit def conv(implicit c: Ctx): Out = new Out("ok")
  implicit def other(implicit c: Ctx): String = "other"
  def plain: String = "plain"
}
