// Function values: eta-expansion of methods (with and without parameter
// lists), placeholder syntax, currying and uncurrying, tupled, compose, and
// methods with multiple parameter lists partially applied.
object Main {
  def add(a: Int, b: Int): Int = a + b
  def addC(a: Int)(b: Int): Int = a + b
  def three(a: Int, b: String, c: Double): String = s"$a$b$c"
  def noArgs(): String = "called"
  var counter = 0
  def effect: Int = { counter += 1; counter }
  def main(args: Array[String]): Unit = {
    val f = add _
    val g = addC(10) _
    val h: Int => Int = addC(100)
    println(f(1, 2) + " " + g(5) + " " + h(1) + " " + f.curried(3)(4) + " " + Function.uncurried((addC _))(1, 1) + " " + f.tupled((6, 7)))
    val t3 = three _
    println(t3(1, "b", 2.5) + " " + t3.curried(1)("x")(0.5) + " " + (three(_: Int, "fixed", _: Double))(9, 1.0))
    val p1 = (_: Int) * 2; val p2 = (_: Int) + (_: Int); val p3 = (_: String).length
    println(p1(4) + " " + p2(1, 2) + " " + p3("four"))
    val incThenDouble = ((x: Int) => x + 1) andThen (_ * 2)
    val doubleThenInc = ((x: Int) => x + 1) compose ((x: Int) => x * 2)
    println(incThenDouble(5) + " " + doubleThenInc(5))
    val na = noArgs _
    println(na())
    val pipeline = List[Int => Int](_ + 1, _ * 10, _ - 3).reduce(_ andThen _)
    println(pipeline(2))
    val chained = Function.chain(Seq((x: Int) => x + 1, (x: Int) => x * x))
    println(chained(3))
    val eff = () => effect
    println(eff() + eff() + " counter=" + counter)
    val byNameFn: (=> Int) => Int = x => x + x
    println(byNameFn(effect) + " counter=" + counter)
    val m = List(1, 2, 3).map(add(1, _))
    println(m)
    val fold = List(1, 2, 3).foldLeft(0) _
    println(fold(_ + _))
    val sq: Double => Double = math.pow(_, 2)
    println(sq(3))
    val sqrt: Double => Double = math.sqrt
    println(sqrt(16))
    val maxf = math.max(_: Int, _: Int)
    println(maxf(3, 9))
    val nested = (a: Int) => (b: Int) => (c: Int) => a * 100 + b * 10 + c
    println(nested(1)(2)(3))
    val constFn = Function.const[Int, String](7) _
    println(constFn("ignored"))
    val applied = List("a", "bb").map(_.length).map(Integer.toString(_, 2))
    println(applied)
    def higher(f: (Int, Int) => Int): Int = f(6, 7)
    println(higher(_ * _) + " " + higher(add) + " " + higher(math.min))
    val tupledFn: ((Int, Int)) => Int = (add _).tupled
    println(List((1, 2), (3, 4)).map(tupledFn))
  }
}
