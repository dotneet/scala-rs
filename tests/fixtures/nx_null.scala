// `Null` erases to `scala/runtime/Null$` in every position but an array
// element, exactly as `Nothing` erases to `scala/runtime/Nothing$`. One
// compilation unit, so nothing here can fail on a descriptor mismatch -- what
// it checks is that the class is reachable in both modes (the private runtime
// has to ship a `Null$` of its own) and that the values still behave.
object Main {
  class Box(val v: Null)

  def n: Null = null
  def take(x: Null): Int = if (x == null) 1 else 2
  def arr: Array[Null] = new Array[Null](0)
  def id(x: Null): Null = x

  def main(args: Array[String]): Unit = {
    println(n)
    println(take(null))
    println(arr.length)
    println(new Box(null).v)
    println(id(null))
  }
}
