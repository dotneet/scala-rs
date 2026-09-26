// SAM literals and `LambdaMetafactory`. nsc implements a SAM type through an
// `invokedynamic` whenever the type compiles to a pure interface -- a Java
// functional interface, or a trait with no fields and no initializer -- and
// through an anonymous class otherwise (a trait with a concrete member or a
// `val`, an abstract class). The literals below cover primitive, wide and
// reference parameters, `void` and boxed results, captured locals and `this`,
// a `Serializable` trait read back through `$deserializeLambda$`, and the
// shapes that must stay classes.
import java.io._
import java.util.function.{IntPredicate, LongUnaryOperator, Predicate, UnaryOperator}

trait Transform { def apply(s: String): String }
trait Gen[A] { def run(a: A): A; def twice(a: A): A = run(run(a)) }
trait Ser extends Serializable { def h(x: Int): Int }
trait WithField { val k = 10; def f(x: Int): Int }
abstract class Abs { def g(x: Int): Int }

class Box(val base: Int) {
  def adder: Gen[Int] = x => x + base
}

object Main {
  def main(args: Array[String]): Unit = {
    val suffix = "!"
    var count = 0
    val t: Transform = s => s + suffix
    val g: Gen[Int] = x => x * 3
    val gs: Gen[String] = s => s.reverse
    val p: Predicate[Integer] = x => x % 2 == 0
    val u: UnaryOperator[String] = x => x + "u"
    val ip: IntPredicate = x => x > 0
    val lu: LongUnaryOperator = x => x * 1000000000L
    val c: java.util.Comparator[String] = (a, b) => a.length - b.length
    val r: Runnable = () => count += 1
    val call: java.util.concurrent.Callable[Int] = () => count + 40
    val w: WithField = x => x + 1
    val a: Abs = x => x * 2
    val s: Ser = x => x + 100

    r.run(); r.run()
    println(List(t("a"), g.run(2), g.twice(2), gs.run("abc"), p.test(4), p.test(5)))
    println(List(u.apply("x"), ip.test(-1), lu.applyAsLong(3L), c.compare("aaa", "b"), call.call()))
    println(List(w.f(1), a.g(4), s.h(1), new Box(7).adder.run(1)))
    val bytes = new ByteArrayOutputStream()
    val out = new ObjectOutputStream(bytes)
    out.writeObject(s)
    out.close()
    val back = new ObjectInputStream(new ByteArrayInputStream(bytes.toByteArray))
      .readObject().asInstanceOf[Ser]
    println(back.h(2))
    val sorted = new java.util.ArrayList[String](java.util.Arrays.asList("ccc", "a", "bb"))
    sorted.sort(c)
    println(sorted)
  }
}
