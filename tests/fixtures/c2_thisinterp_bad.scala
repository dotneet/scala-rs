// Only `this` is a keyword the `$`-form of an interpolation understands: an
// ordinary name that is not in scope is still "not found", so the fix does not
// turn every `$name` into a silent success.

class Node(val name: String) {
  def describe(n: Int): String = s"$n of $nosuchvalue"
}

object Main {
  def main(args: Array[String]): Unit = println(new Node("a").describe(1))
}
