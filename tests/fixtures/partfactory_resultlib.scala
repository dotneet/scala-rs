trait ResultEvidence[F[_]] { def pure[A](a: A): F[A] }
class ResultResource[F[_], A](a: A) {
  def allocated[B >: A](implicit ev: ResultEvidence[F]): F[(B, F[Unit])] =
    ev.pure((a, ev.pure(())))
}
class ResultCollection[A](a: A) {
  def values[B >: A]: Set[B] = Set(a)
}
