object Main {
  case class Show[A](text: String)
  implicit def show[A](implicit m: Manifest[A]): Show[A] = Show(m.toString)
  def go[A](implicit s: Show[A]): String = s.text
  def use[F[_, _]](f: F[Int, String]): F[Int, String] = f
  def optional[A](implicit m: OptManifest[A]): OptManifest[A] = m
  def unknown[A] = optional[A]
  def partial[A] = optional[List[A]]
  def unknownArray[A] = optional[Array[A]]
  def taggedArray[A](implicit tag: scala.reflect.ClassTag[A]) = optional[Array[A]]
  trait P
  trait Q
  def main(args: Array[String]): Unit = {
    val f: Function[Int, String] = _.toString
    println(use[Function](f)(3))
    println(Function.const[String, Int]("ok")(1))
    println(List(1, 2).collect(Function.unlift((x: Int) => if (x == 2) Some(x) else None)))
    println(Function.tupled((x: Int, y: Int) => x + y)((2, 3)))
    println(manifest[Int]); println(optManifest[Int]); println(Manifest.Int); println(NoManifest)
    println(go[List[Int]])
    println(optional[Int]); println(optional[List[String]])
    println(unknown[Int]); println(partial[Int]); println(optional[Array[Int]])
    println(unknownArray[Int]); println(taggedArray[Int]); println(optional[P with Q])
  }
}
