// `Unit` as a *type argument* erases to `BoxedUnit` too: `Array[Unit]` is
// `[Lscala/runtime/BoxedUnit;`, and `List[Unit]` / `Option[Unit]` /
// `Map[String, Unit]` / `Set[Unit]` / `PartialFunction[Int, Unit]` hold the
// singleton. Needs the real scala-library: the private runtime has no
// `List.apply`/`Array.apply` for a varargs literal, no `Map`/`Set`, and no
// `Function2` at all.
object Main {
  def arr: Array[Unit] = Array((), ())
  def opt: Option[Unit] = Some(())
  def varargs(us: Unit*): Int = us.length

  def main(args: Array[String]): Unit = {
    println(varargs((), (), ()))
    opt.foreach(u => println(u))
    println(opt.map(u => u).isDefined)
    println(List((), ()).length)
    println(List((), ()))
    println(arr.length)
    // Indexed through a `val`: `def a: Array[T]` followed by `a(0)` needs an
    // `apply` insertion this compiler does not do yet, for every element type.
    val a: Array[Unit] = arr
    println(a(0))
    println(opt)
    println(opt.get)
    println(Seq(()).head)
    println(((), 1))
    // A lambda whose result is `Unit` still returns a reference through
    // `Function1.apply`, so the elements really are `BoxedUnit`.
    println(List(1, 2).map(_ => ()))
    println(List(1, 2).map(_ => ()).length)
    val mp: Map[String, Unit] = Map("a" -> ())
    println(mp("a"))
    println(mp)
    val st: Set[Unit] = Set(())
    println(st)
    println(List(()).headOption)
    val pf: PartialFunction[Int, Unit] = { case 1 => () }
    println(pf(1))
    println(List(1).foreach(_ => ()))
    // `Function2`: the private runtime only has `Function0` / `Function1`.
    val f2: (Unit, Int) => String = (u, n) => "f" + n
    println(f2((), 1))
  }
}
