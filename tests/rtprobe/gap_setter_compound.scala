// scala-rs rejects: `c.x += n` where x is a getter with a hand-written
// `x_=` setter (scalac desugars it to `c.x = c.x + n`).
object Main {
  class CustomSetter { private var _x = 0; def x = _x; def x_=(n: Int): Unit = { _x = if (n < 0) 0 else n } }
  def main(args: Array[String]): Unit = {
    val cs = new CustomSetter
    cs.x = 5
    cs.x += 7; println(cs.x)
    cs.x -= 100; println(cs.x)
  }
}
