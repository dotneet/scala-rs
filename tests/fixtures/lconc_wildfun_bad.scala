// The other direction: what the `_ => _` leniency must NOT start accepting.
// An existential on the *left* is not an instantiation, a pinned result is
// still checked, and a non-function is still not a function.
object Main {
  def wild(f: Function1[_, _]): Int = 1
  def exact(f: Function1[Int, String]): Int = 2
  def retS(f: Function1[_, String]): Int = 3

  def main(args: Array[String]): Unit = {
    val w: Function1[_, _] = (i: Int) => "x"
    println(exact(w))
    println(retS((i: Int) => 1))
    println(wild(42))
  }
}
