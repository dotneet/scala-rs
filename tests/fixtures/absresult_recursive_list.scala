trait Foo[A] { def value: String }
object Main {
  implicit val intFoo: Foo[Int] = new Foo[Int] { def value = "int" }
  implicit def listFoo[A](implicit f: Foo[A]): Foo[List[A]] =
    new Foo[List[A]] { def value = f.value }
  val f = implicitly[Foo[List[List[List[List[List[List[List[List[List[Int]]]]]]]]]]]
  def main(args: Array[String]): Unit = println(f.value)
}
