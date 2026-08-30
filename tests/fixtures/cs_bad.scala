// A template-body statement is part of the initializer, so it is type-checked
// like any other code — it is never quietly discarded.
object Main {
  class A {
    println("ok")
    notAMethod(1)
  }
  trait T {
    val n: Int
    println(n.noSuchMember)
  }
  def main(args: Array[String]): Unit = {
    new A
    ()
  }
}
