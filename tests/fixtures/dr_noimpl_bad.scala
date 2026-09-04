// A method taking only implicits is not a value. If they cannot be filled it is a
// type error -- it must not be quietly eta-expanded into a function value (this
// used to make `println(List(Some(1), None, Some(3)).flatten)` print
// `Main$$$anonfun$0@7a765367`).
// Real scalac fails with "could not find implicit value for parameter m" too.
trait Marker[A] { def tag: String }

object Main {
  def widget[A](implicit m: Marker[A]): String = m.tag

  def main(args: Array[String]): Unit = {
    println(widget)
  }
}
