trait TC[A]
trait W[A] { implicit def algebra: TC[A] }
object Main {
  def make[A](implicit x: TC[A], y: TC[A]): W[A] =
    new W[A] { val algebra = implicitly[TC[A]] }
}

