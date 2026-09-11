// scalac: value secret in class A cannot be accessed.
object Main {
  class A { private val secret = 42 }
  def main(args: Array[String]): Unit = println(new A().secret)
}
