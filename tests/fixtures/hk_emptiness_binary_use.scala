import hkemptiness._
import hkemptiness.Instances._

object Main {
  def selected[T](implicit emptiness: Emptiness[T]): String = emptiness.label

  def main(args: Array[String]): Unit = {
    println(selected[Option[String]])
    println(selected[List[String]])
    println(selected[java.util.ArrayList[String]])
  }
}
