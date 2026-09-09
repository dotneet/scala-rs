class Narrow extends Broad
class Broad
trait Parent {
  def choose(a: Narrow): Narrow
  def choose(a: Broad): Broad
}
class Child extends Parent {
  override def choose(a: Broad) = a
  override def choose(a: Narrow) = a
}
object Main {
  def main(args: Array[String]): Unit = {
    val c = new Child
    val b = new Broad
    val n = new Narrow
    println(c.choose(b) eq b)
    println(c.choose(n) eq n)
  }
}
