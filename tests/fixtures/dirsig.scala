import dirsig._
object Main {
  implicit val ev: Evidence[Option] = new Evidence[Option] { def pure[A](a: A): Option[A] = Some(a) }
  implicit val prefix: String = "prefix:"
  def main(args: Array[String]): Unit = {
    println(new Derived("inherited").allocated.map(_._1))
    val r = new Resource[Option, String]("ok")
    println(r.allocated.map(_._1.length))
    println(r.allocated[Any].map(_._1))
    println(r.combine("tail"))
  }
}
