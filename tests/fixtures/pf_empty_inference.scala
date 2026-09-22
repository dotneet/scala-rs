import scala.concurrent.{ExecutionContext, Future}

final case class Program(programId: Int)

object Main {
  implicit val ec: ExecutionContext = ExecutionContext.global

  def load: Future[Seq[Program]] = ???

  def run: Future[Option[Program]] = for {
    programs <- load.recover { case _ => Seq.empty }
    program = programs.find(_.programId == 1)
  } yield program
}
