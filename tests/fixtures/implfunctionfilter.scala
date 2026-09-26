trait Read[+A] extends (Int => A)
trait Marker { def label: String }
trait Missing
trait Evidence { def value: Int }

object Main {
  implicit def unrelated(implicit missing: Missing): Read[Double] =
    new Read[Double] { def apply(n: Int): Double = n.toDouble }
  implicit val evidence: Evidence = new Evidence { def value: Int = 42 }

  object Readers {
    implicit val reader: Read[String] = new Read[String] {
      def apply(n: Int): String = n.toString
    }
    def result: String = {
      val f = implicitly[Int => String]
      f(42)
    }
  }

  class Tagged extends Read[String] with Marker {
    def apply(n: Int): String = n.toString
    def label: String = "tag"
  }
  object Tags {
    implicit val tagged: Tagged = new Tagged
    def result: String = implicitly[Marker].label
  }

  def main(args: Array[String]): Unit = {
    println(implicitly[Evidence].value)
    println(Readers.result)
    println(Tags.result)
  }
}
