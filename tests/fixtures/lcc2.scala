// A local `case object` (as opposed to `case class`) has no separate
// synthetic companion to lose: the object declaration itself already goes
// through `emit_module` in the `Block` arm of `emit_anon_classes`. This
// fixture pins that down so a future change to the case-class companion path
// cannot silently break the object form instead.
object Main {
  def main(a: Array[String]): Unit = {
    case object Red
    case object Blue
    println(Red)
    println(Blue)
    println(Red == Red)
    println(Red equals Blue)
  }
}
