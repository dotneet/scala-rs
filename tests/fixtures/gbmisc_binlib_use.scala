// Against `tests/fixtures/gbmisc_binlib/GbmiscLib.scala`, compiled by scalac.
//
// * `html.edithook(...)`: an object extending a binary case class. The JVM
//   `static apply` forwarder on that class is no member of the object;
//   counted as a second `apply` it withheld the parameter types from the
//   arguments, and `Set(Ev.Push)` missed the invariant `Set[Ev]`
//   (gitbucket's Twirl templates, `html.edithook(hook, Set(WebHook.Push),
//   account, create = true)`).
// * `super.rp(u, i)`: the pickled *abstract* `SB.rp`, reached through `FM`
//   first, is not the member `super` means; the concrete overloads of `SF`
//   are (gitbucket's `ControllerBase.requestPath`).
import gblib._

sealed trait Ev
object Ev { case object Push extends Ev }
package html {
  object edithook extends Base[String]("h") {
    def apply(a: Int, events: Set[Ev], create: Boolean): String = s"$a $events $create $format"
  }
}
class CB extends SF with FM {
  override def rp(u: String, i: Int): String = "cb" + super.rp(u, i)
}
object Main {
  def main(args: Array[String]): Unit = {
    println(html.edithook(1, Set(Ev.Push), create = true))
    println(html.edithook(2, Set(Ev.Push), false))
    println(new CB().rp("u", 2))
  }
}
