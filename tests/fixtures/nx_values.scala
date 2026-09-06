class NxValuesBox[T](val value: T)

object Main {
  var calls = 0
  def n: Null = { calls += 1; null }
  def asString: String = n
  def nullParam(n: Null): String = n
  def take(n: Null): Boolean = n == null
  def main(args: Array[String]): Unit = {
    println(asString)
    println(nullParam(n))
    val xs: Array[Null] = new Array[Null](1)
    println(take(xs(0)))
    println(xs.getClass.getName)
    val ys = new NxValuesBox[Null](null)
    println(take(ys.value))
    println(calls)
    try { "bad".asInstanceOf[Null]; println("wrong") }
    catch { case _: ClassCastException => println("cast") }
  }
}
