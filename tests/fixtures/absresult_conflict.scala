trait Factory[+C] { def empty: C }
class Base extends Factory[Base] { def empty: Base = new Base }
class Child extends Base with Factory[Child] {
  override def empty: Child = new Child
}
trait Wide { def text: Any }
class Provider { def text: String = "inherited" }
class Valid extends Provider with Wide
object Main {
  def main(args: Array[String]): Unit = {
    val factory: Factory[Child] = new Child
    println(factory.empty.isInstanceOf[Child])
    val wide: Wide = new Valid
    println(wide.text)
  }
}
