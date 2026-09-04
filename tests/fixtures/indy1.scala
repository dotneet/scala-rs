// Lambdas that become `invokedynamic` + `LambdaMetafactory`, in the shapes
// the private runtime can also carry: `Function0` and `Function1` only.
trait Bump {
  def base: Int
  def bumped(k: Int): Int = {
    val f: Int => Int = i => i + base
    f(k)
  }
}

class Holder(val base: Int) extends Bump {
  def scaled(d: Int): Int = {
    val f: Int => Int = i => i * base + d
    f(2)
  }
  def nested(d: Int): Int = {
    val outer: Int => Int = i => {
      val inner: Int => Int = j => j + i + d + base
      inner(1)
    }
    outer(10)
  }
}

object Main {
  val plain: Int => Int = (x: Int) => x + 1
  val thunk: () => Int = () => 42

  def adder(n: Int): Int => Int = (x: Int) => x + n

  def twice(f: Int => Int, v: Int): Int = f(f(v))

  def summed(n: Int): Int = {
    var acc = 0
    val add: Int => Unit = i => acc = acc + i
    var i = 0
    while (i < n) { add(i); i = i + 1 }
    acc
  }

  def strings(s: String): String = {
    val f: String => String = x => x + "!"
    f(s)
  }

  def main(args: Array[String]): Unit = {
    println(plain(1))
    println(thunk())
    println(adder(10)(5))
    println(twice(x => x * 3, 2))
    println(summed(5))
    println(strings("ok"))
    println(new Holder(3).scaled(4))
    println(new Holder(3).nested(0))
    println(new Holder(7).bumped(1))
    println(Some(4).map(x => x + 1).getOrElse(0))
  }
}
