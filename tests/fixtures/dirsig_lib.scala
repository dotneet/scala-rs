package dirsig
trait Evidence[F[_]] { def pure[A](a: A): F[A] }
class Resource[F[_], A](a: A) {
  def allocated[B >: A](implicit ev: Evidence[F]): F[(B, F[Unit])] = ev.pure((a, ev.pure(())))
  def combine(s: String)(implicit prefix: String): String = prefix + s
}
class Derived(a: String) extends Resource[Option, String](a)
