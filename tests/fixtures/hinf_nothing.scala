// nsc's `adjustTypeArgs`: a `Nothing` the arguments inferred for a type
// parameter that occurs invariantly or contravariantly in the result is
// *retracted* -- kept undetermined for the expected type or the enclosing
// expression to decide -- and only a covariant occurrence instantiates it.
// `inv(fail())` is an `Inv[?T]` for the following `.or("a")`; the rule used
// to be spelled for `tryBreakable` by name.
object Main {
  class Inv[T](body: () => T) {
    def or(u: T): T = try body() catch { case _: Throwable => u }
    def get: T = body()
  }
  def inv[T](body: => T): Inv[T] = new Inv(() => body)
  def mk[T](t: T): Inv[T] = new Inv(() => t)
  class Contra[-T] { def take(t: T): String = "took " + t }
  def contra[T](body: => T): Contra[T] = new Contra[T]
  def fn[T](x: => T): T => String = t => "fn " + t
  def fail(): Nothing = throw new RuntimeException("no")
  def main(args: Array[String]): Unit = {
    println(inv(fail()).or("a"))
    println(inv(throw new RuntimeException("x")).or(1))
    println(inv(???).or(2.5))
    println(contra(fail()).take(1))
    val g: String => String = fn(fail())
    println(g("z"))
    // Covariant results keep Nothing (nsc: Some[Nothing], List[Nothing]).
    val o = Option(fail _)
    val l: List[Int] = List()
    println((o.isDefined, l))
    // The expected type decides the retracted variable, or keeps Nothing.
    val z: Inv[Int] = inv(fail())
    val w: Inv[Nothing] = inv(fail())
    println(z.or(3))
    println(w.getClass.getSimpleName)
    // A definition closes it at Nothing (nsc's mono-mode instantiate).
    val y = inv(fail())
    println(y.getClass.getSimpleName)
    val c: Inv[Inv[String]] = mk(inv(fail()))
    println(c.get.or("chained"))
    def both[T](a: => T, b: T): T = b
    println(both(fail(), "b"))
  }
}
