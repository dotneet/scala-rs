// Every line here is rejected by real scalac 2.13.16 as well: `new` needs a
// class type, and neither a type parameter nor an abstract type member is
// one.
trait Named {
  type Self
  def make = new Self
}
object Main {
  def f[T] = new T
  def main(args: Array[String]): Unit = {
    println(f[Int])
  }
}
