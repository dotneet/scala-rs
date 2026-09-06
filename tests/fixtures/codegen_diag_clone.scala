// A generic array clone has no private-runtime implementation. The backend
// must diagnose that limitation instead of returning a class with a stub.
object Main {
  def dup[T](a: Array[T]): Array[T] = a.clone()

  def main(args: Array[String]): Unit = {
    val a = new Array[Int](0)
    println(dup(a).length)
  }
}
