trait Backend {
  def name: String = "backend"
  def describe: String = "I am " + name
}
object Backend extends Backend {
  override def name: String = "the object"
}
trait Rep[T] { def value: T }
class IntRep(val value: Int) extends Rep[Int]
object Main {
  def show[A <: Rep[Int]](r: A): Int = r.value
  def main(args: Array[String]): Unit = {
    println(Backend.describe)
    println(show(new IntRep(7)))
    val nested: Option[Option[Int]] = Some(Some(5))
    println(nested)
  }
}
