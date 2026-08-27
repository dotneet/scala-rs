import scala.math.Ordering
class C[T: Ordering](val x: T)
object Main {
  def main(args: Array[String]): Unit = {
    println(new C(2).x)
    println(new C(1).x)
  }
}
