class Z[A](val value: LazyList[A]) extends AnyVal
trait ZipApplicative[F[_]] {
  def ap[A,B](ff:F[A=>B])(fa:F[A]):F[B]
  def map[A,B](fa:F[A])(f:A=>B):F[B]
}
object Z {
  def apply[A](a:LazyList[A]):Z[A]=new Z(a)
  val instance: ZipApplicative[Z] = new ZipApplicative[Z] {
    def map[A,B](fa:Z[A])(f:A=>B):Z[B]=Z(fa.value.map(f))
    def ap[A,B](ff:Z[A=>B])(fa:Z[A]):Z[B]=Z(ff.value.lazyZip(fa.value).map(_.apply(_)))
  }
}
object Main {
 def main(args:Array[String]):Unit=println(Z.instance.ap(Z(LazyList((i:Int)=>i+1)))(Z(LazyList(4))).value.head)
}
