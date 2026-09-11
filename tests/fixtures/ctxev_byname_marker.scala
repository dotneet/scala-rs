class `<ByName>`[A](val value: A)
object Main {
  var n = 0
  def main(args: Array[String]): Unit = {
    val x: `<ByName>`[Int] = new `<ByName>`(7)
    println(x.value)
    val f: (⇒ Int) ⇒ Int = x ⇒ x + x
    println(f({ n += 1; n }))
    println(n)
  }
}
