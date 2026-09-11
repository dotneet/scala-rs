// scalac: value x is not a member of Main.C -- private[this] is not visible
// through another instance of the same class.
object Main {
  class C { private[this] val x = 1; def peek(o: C): Int = o.x }
  def main(args: Array[String]): Unit = println(new C().peek(new C))
}
