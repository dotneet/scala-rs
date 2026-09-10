package implicitidentity
trait Evidence[A] { def value:Int }
object Evidence {
 implicit object intEvidence extends Evidence[Int] { def value:Int=11 }
}
trait Provider {
 def seed:Int
 implicit object stringEvidence extends Evidence[String] { def value:Int=seed }
}
class Holder(val seed:Int) extends Provider
object Inherited extends Provider { def seed:Int=33 }
object Values {
 implicit val doubleEvidence:Evidence[Double]=new Evidence[Double] { def value:Int=44 }
 implicit def longEvidence:Evidence[Long]=new Evidence[Long] { def value:Int=55 }
}
object Twins {
 implicit object first extends Evidence[Boolean] { def value:Int=1 }
 implicit object second extends Evidence[Boolean] { def value:Int=2 }
}
object Hidden {
 private implicit object hidden extends Evidence[Char] { def value:Int=9 }
}
