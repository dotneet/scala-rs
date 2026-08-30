// Rule 8: the number of type parameters is part of the signature, so a member
// with a different count overrides nothing.
//
// scalac 2.13.16: "method f overrides nothing."
object Main {
  class B { def f[A](x: A): A = x }
  class D extends B { override def f[A, C](x: A): A = x }
  def main(args: Array[String]): Unit = println(new D().f[Int, Int](1))
}
