// A type projection `Base#Inner` takes its enclosing class's abstract
// `T` as one unknown instance's `T` in parameter positions (`neg/sabin2`):
// nothing but `Nothing` conforms. Error lines are listed in
// `crates/cli/tests/libapp.rs`.
object Test {
  abstract class Base {
    type T
    var x: T = _
    class Inner {
      def set(y: T) = x = y
      def get() = x
      def pick(y: T): String = "T"
      def pick(y: String): String = "String"
    }
    def m2(i: Base#Inner, t: T): Unit = i.set(t)
  }
  object IntBase extends Base { type T = Int }

  val a: Base#Inner = new IntBase.Inner
  val b: Base#Inner = new IntBase.Inner
  val n = new IntBase.Inner

  def bad(): Unit = {
    a.set(b.get())
    a.set(a.get())
    a.set(null)
    a.set(1)
    val t: Base#T = 1
    val u: Int = a.get()
    a.pick(a.get())
    n.set("s")
  }
}
object Main { def main(args: Array[String]): Unit = () }
