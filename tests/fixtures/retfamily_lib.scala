package family
class Rich[A](val x: A) { def returning: A = x }
trait Root {
  type Out[A]
  def make[A](x: A): Out[A]
  trait Api { implicit def convert[A](x: A): Out[A] = make(x) }
}
class Derived extends Root {
  type Out[A] = Rich[A]
  trait DerivedApi extends Api
  val api: DerivedApi = new DerivedApi {}
  def make[A](x: A): Rich[A] = new Rich(x)
}
