// A field of type `Unit` is a `scala/runtime/BoxedUnit` slot; only a method
// *result* is `V`. Getters, setters, `lazy val` and trait vals all have to
// agree with the field's descriptor.
trait T {
  val tv: Unit = ()
  var tw: Unit = ()
}

class C extends T {
  val cv: Unit = ()
  var cw: Unit = ()
  lazy val cl: Unit = ()
  def bump(): Unit = { cw = (); tw = () }
}

object Main {
  val ov: Unit = ()
  var ow: Unit = ()
  lazy val ol: Unit = ()

  def main(args: Array[String]): Unit = {
    val c = new C
    println(c.cv)
    println(c.cw)
    println(c.cl)
    println(c.tv)
    println(c.tw)
    c.bump()
    println(c.cw)
    println(ov)
    println(ow)
    ow = ()
    println(ow)
    println(ol)
    var local: Unit = ()
    local = ()
    println(local)
    val any: Any = ()
    println(any)
  }
}
