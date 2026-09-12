// `_ => _` as a parameter type: an existential `Function1[_, _]`, which every
// function conforms to. `scala/concurrent/impl/Promise.scala` declares
// `Transformation`'s constructor that way (`def this(xform: Int, f: _ => _, ec:
// ExecutionContext)`) and hands it eleven differently typed closures -- a
// `Try[T] => Try[S]`, a `T => Boolean`, a `PartialFunction[Throwable, U]`, ...
object Main {
  class Box(val tag: String, f: _ => _) {
    val fun: Any => Any = f.asInstanceOf[Any => Any]
  }

  // A wildcard in the *parameter* position only, the result pinned.
  def retTyped(f: Function1[_, String]): Boolean = f ne null

  def main(args: Array[String]): Unit = {
    println(new Box("m", (i: Int) => "v" + i).fun(1))
    val pf: PartialFunction[Int, String] = { case 1 => "one" }
    println(new Box("p", pf).fun(1))
    println(new Box("u", (s: String) => ()).fun("z"))
    println(retTyped((i: Int) => "x" + i))
    println(retTyped((s: String) => s.toUpperCase))
  }
}
