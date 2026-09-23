package nestedhk

trait PolymorphicApply[Tuple[_[_]], Result[_[_]]] {
  def tupledApply[F[_]](tuple: Tuple[F]): Result[F]
}
