trait TT[T] { def name: String }
object TT {
  implicit val ttInt: TT[Int] = new TT[Int] { def name = "Int" }
  implicit val ttStr: TT[String] = new TT[String] { def name = "String" }
}

class Parent[T: TT] {
  def describe: String = "p[" + implicitly[TT[T]].name + "]"
}
