object Main {
  implicit val ev: ResultEvidence[Option] = new ResultEvidence[Option] {
    def pure[A](a: A): Option[A] = Some(a)
  }
  def main(args: Array[String]): Unit = {
    val r = new ResultResource[Option, String]("ok")
    println(r.allocated.map(_._1.length))
    println(r.allocated[Any].map(_._1))
    val c = new ResultCollection[String]("ok")
    val values = c.values
    println(values.head.length)
    println(c.values[Any].head)
  }
}
