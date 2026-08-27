class NotSam {
  def a(): Unit = ()
  def b(): Unit = ()
}
trait TwoAbs {
  def a(): Unit
  def b(): Unit
}
object Main {
  def go(): Unit = ()
  val x: NotSam = () => ()
  val y: TwoAbs = () => ()
  val z: Runnable = go
}
