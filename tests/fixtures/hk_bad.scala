class Id[A](val value: A)
trait Functor[F[_]] { def dummy: Int }
object Main {
  def asProper[F[_]](x: F): Unit = ()
  def useFunctor(x: Functor[Int]): Unit = ()
  def notCtor[A](x: Int[A]): Unit = ()
  def main(args: Array[String]): Unit = ()
}
