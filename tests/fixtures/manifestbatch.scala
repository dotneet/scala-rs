object Main {
  def m[A](implicit ev: scala.reflect.Manifest[A]): scala.reflect.Manifest[A] = ev
  class Box[A]
  class Self {
    def same: Boolean = m[this.type] == scala.reflect.Manifest.singleType(this)
  }
  trait P
  trait Q
  object Key { override def toString: String = "key" }
  def array[A](implicit ev: scala.reflect.Manifest[A]): scala.reflect.Manifest[Array[A]] = m[Array[A]]
  def list[A](implicit ev: scala.reflect.Manifest[A]): scala.reflect.Manifest[List[A]] = m[List[A]]
  def main(args: Array[String]): Unit = {
    println(m[Byte]); println(m[Short]); println(m[Char]); println(m[Int])
    println(m[Long]); println(m[Float]); println(m[Double]); println(m[Boolean])
    println(m[Unit]); println(m[Any]); println(m[AnyRef]); println(m[AnyVal])
    println(m[Nothing]); println(m[Null]); println(m[String])
    println(m[List[String]]); println(m[Map[String, Int]]); println(m[Box[Int]])
    println(m[Array[Int]]); println(m[Array[Array[String]]]); println(array[Int])
    println(list[Int]); println(m[(String, Int)]); println(m[Int => String])
    println(m[Key.type] == scala.reflect.Manifest.singleType(Key))
    println(new Self().same)
    println(m[P with Q])
    println(m[List[_]]); println(m[List[_ <: Number]]); println(m[List[_ >: String <: AnyRef]])
  }
}
