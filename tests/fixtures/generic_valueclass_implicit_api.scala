package genericvalueclass

trait Reader[A] { def read: A }

final class Exported[A](val value: A) extends AnyVal

object Reader {
  implicit def exported[A](implicit value: Exported[Reader[A]]): Reader[A] = value.value
}

object Auto {
  implicit val stringReader: Exported[Reader[String]] =
    new Exported(new Reader[String] { def read: String = "ok" })
}
