import scala.reflect.runtime.universe._
class Box[+A](val t: Type)
object Main {
  def weak[A: WeakTypeTag](label: String): Box[A] = new Box(weakTypeOf[A])
  def strong[A: TypeTag](label: String): Box[A] = new Box(typeOf[A])
  def lower[A >: String : WeakTypeTag](label: String): Box[A] = new Box(weakTypeOf[A])
  def scoped[A: WeakTypeTag]: Box[A] = weak[A]("explicit")
  def main(args: Array[String]): Unit = {
    val w: Box[Int] = weak("weak")
    val s: Box[Int] = strong("strong")
    val l: Box[AnyRef] = lower("lower")
    println(w.t =:= typeOf[Nothing])
    println(s.t =:= typeOf[Nothing])
    println(l.t =:= typeOf[String])
    println(scoped[Int].t =:= typeOf[Int])
  }
}
