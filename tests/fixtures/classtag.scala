import scala.reflect.ClassTag
object Main {
  def mk[T: ClassTag](n: Int): Array[T] = new Array[T](n)
  def main(args: Array[String]): Unit = {
    println(implicitly[ClassTag[Int]].runtimeClass.getName)
    println(mk[Int](2).length)
  }
}
