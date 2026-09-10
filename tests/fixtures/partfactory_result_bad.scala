object Main {
  implicit val ev: ResultEvidence[Option] = new ResultEvidence[Option] {
    def pure[A](a: A): Option[A] = Some(a)
  }
  val r = new ResultResource[Option, String]("ok")
  val bad = r.allocated[Any].map(_._1.length)
}
