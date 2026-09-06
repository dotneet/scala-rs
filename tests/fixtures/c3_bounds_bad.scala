trait UpperApply { def apply[A <: java.lang.Number](a: A): A }
trait Box[A]
trait OwnerApply[T] { def apply[A <: T](a: A): A }
trait LowerApply[T] { def apply[A >: T](a: Box[A]): Box[A] }
object BoundsBad {
  def upper: UpperApply = null
  def owner: OwnerApply[java.lang.Number] = null
  def lower: LowerApply[String] = null
  val a: String = upper("wrong")
  val b: String = owner("wrong")
  val c: Box[Int] = lower(null: Box[Int])
}
