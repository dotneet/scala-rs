import hkemptiness._
import hkemptiness.OnlyIterable._

object Bad {
  val impossible: Emptiness[Option[String]] = implicitly[Emptiness[Option[String]]]
}
