// scalac: method f in class A cannot be accessed ... Access to protected
// method f not permitted because enclosing object Main is not a subclass.
object Main {
  class A { protected def f: Int = 1 }
  def main(args: Array[String]): Unit = println(new A().f)
}
