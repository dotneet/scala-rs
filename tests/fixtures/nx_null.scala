// `Null` erases to `scala/runtime/Null$` in every position but an array
// element, exactly as `Nothing` erases to `scala/runtime/Nothing$`. One
// compilation unit, so nothing here can fail on a descriptor mismatch -- what
// it checks is that the class is reachable in both modes (the private runtime
// has to ship a `Null$` of its own), that the values still behave, and that
// an *erasure bridge* down to one of the two bottom classes says so: the
// bridge's parameter arrives as `Object`, which is not assignable to `Null$`,
// and without the `checkcast` nsc emits the whole class is a `VerifyError`.
abstract class NxBase[T] {
  def lit(v: T): String
  def get: T
}

class NxNull extends NxBase[Null] {
  def lit(v: Null): String = "NULL"
  def get: Null = null
}

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
    // Through the erased parent, so the calls go through the bridges.
    val b: NxBase[Null] = new NxNull
    println(b.lit(null))
    println(b.get)
  }
}
