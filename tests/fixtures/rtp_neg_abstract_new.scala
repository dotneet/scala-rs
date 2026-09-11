// scalac: class A is abstract; cannot be instantiated
object Main {
  abstract class A { def f: Int }
  def main(args: Array[String]): Unit = println(new A)
}
