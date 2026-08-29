// A lambda that reads the enclosing `this` — written out or, far more often,
// left implicit in a call to a method of the enclosing class — has to capture
// it. Without the capture the lambda class gets no `$outer` and codegen loads
// slot 0, which inside `apply` is the *lambda*: `C$$anonfun$0 cannot be cast
// to C` at run time, with the type checker perfectly happy.

trait L1 {
  def base(n: Int): Int
  // the reported shape: a lambda inside a trait's default method
  def viaLambda(xs: List[Int]): List[Int] = xs.map(a => base(a))
  def viaThis(xs: List[Int]): List[Int] = xs.map(a => this.base(a))
}

class M3 extends L1 {
  def base(n: Int): Int = n * 2
}

class C1 {
  val k: Int = 100
  def base(n: Int): Int = n + 1
  def implicitThis(xs: List[Int]): List[Int] = xs.map(a => base(a))
  def explicitThis(xs: List[Int]): List[Int] = xs.map(a => this.base(a))
  // a field read already worked: it is a free *term*, which the old free-variable
  // scan did see. Kept so the two paths stay in step.
  def field(xs: List[Int]): List[Int] = xs.map(a => a + k)
  // nested: the inner lambda's `this` has to travel out through the outer one
  def nested(xs: List[Int]): List[Int] = xs.flatMap(a => List(1).map(b => base(a + b)))
}

object O3 {
  def base(n: Int): Int = n * 3
  // a member of an *object* is reached through `MODULE$`, not through `this`
  def viaModule(xs: List[Int]): List[Int] = xs.map(a => base(a))
}

object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3)
    val m = new M3
    println(m.viaLambda(xs))
    println(m.viaThis(xs))
    val c = new C1
    println(c.implicitThis(xs))
    println(c.explicitThis(xs))
    println(c.field(xs))
    println(c.nested(xs))
    println(O3.viaModule(xs))
  }
}
