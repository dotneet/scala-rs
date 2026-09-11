// scalac: trait T is abstract; cannot be instantiated
object Main {
  trait T { def g: Int = 1 }
  def main(args: Array[String]): Unit = println(new T)
}
