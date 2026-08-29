// The same `$outer` capture as `cats_lambda.scala`, written without any
// library collection so it also runs against the private runtime
// (`--no-scala-library`).

trait L2 {
  def base(n: Int): Int
  def apply2(f: Int => Int, n: Int): Int = f(n)
  // a lambda in a trait's default method, calling an abstract method on `this`
  def viaLambda(n: Int): Int = apply2(a => base(a), n)
  def viaThis(n: Int): Int = apply2(a => this.base(a), n)
}

class M2 extends L2 {
  def base(n: Int): Int = n * 2
}

class C2 {
  val k: Int = 100
  def base(n: Int): Int = n + 1
  def call(f: Int => Int, n: Int): Int = f(n)
  def implicitThis(n: Int): Int = call(a => base(a), n)
  def explicitThis(n: Int): Int = call(a => this.base(a), n)
  def field(n: Int): Int = call(a => a + k, n)
  def nested(n: Int): Int = call(a => call(b => base(a + b), 1), n)
}

object O2 {
  def base(n: Int): Int = n * 3
  def call(f: Int => Int, n: Int): Int = f(n)
  def viaModule(n: Int): Int = call(a => base(a), n)
}

object Main {
  def main(args: Array[String]): Unit = {
    val m = new M2
    println(m.viaLambda(5))
    println(m.viaThis(5))
    val c = new C2
    println(c.implicitThis(5))
    println(c.explicitThis(5))
    println(c.field(5))
    println(c.nested(5))
    println(O2.viaModule(5))
  }
}
