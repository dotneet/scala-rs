// The checks that apply to a top-level mixin apply to a local one too: a
// local trait constrained to a superclass cannot be mixed into a local class
// that does not extend it.
class Sup
trait Constrained extends Sup { def f = "f" }

object Main {
  def main(args: Array[String]): Unit = {
    class Other
    class Bad extends Other with Constrained
    println(new Bad().f)
  }
}
