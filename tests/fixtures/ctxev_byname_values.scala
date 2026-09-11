object Main {
  var forced = 0
  var invoked = 0
  def hold[A](a: => A): A = a
  def twice(f: => () => Int): Int = f() + f()
  def forward(f: => () => Int): Int = twice(f)
  def curried[A](a: => A)(dummy: Int = 0): A = a
  class Parent[A](a: => A) { def get: A = a }
  class Child extends Parent[Int => Int]((x: Int) => x + 1)
  def main(args: Array[String]): Unit = {
    val zero: () => Int = hold(() => 7)
    println(zero())
    println(hold((x: Int) => x + 1)(6))
    println(hold((x: Int, y: Int) => x + y)(3, 4))
    println(hold((x: Int, y: Int, z: Int) => x + y + z)(1, 2, 4))
    println(forward({ forced += 1; () => { invoked += 1; invoked } }))
    println(forced)
    println(invoked)
    println(curried(() => 7)()())
    println(new Child().get(6))
    val values = Map("x" -> (() => "ok"))
    println(values.getOrElse("missing", () => 3)())
    val one = (x: Int) => x + 1
    println(hold(one)(6))
  }
}
