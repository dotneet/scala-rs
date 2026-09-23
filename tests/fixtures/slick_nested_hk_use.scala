import nestedhk.PolymorphicApply
import slick.jdbc.H2Profile.api._

case class Sample[F[_]](id: F[Int], label: F[String])

object Sample {
  type Tuple[F[_]] = (F[Int], F[String])

  val tupled: PolymorphicApply[Tuple, Sample] = new PolymorphicApply[Tuple, Sample] {
    override def tupledApply[F[_]](tuple: Tuple[F]): Sample[F] = (apply[F] _).tupled(tuple)
  }
}

object ShapeUse {
  def applyRepTuple(tuple: Sample.Tuple[Rep]): Sample[Rep] = Sample.tupled.tupledApply(tuple)
}
