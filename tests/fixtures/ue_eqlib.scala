// `Unit` operands of `==` that need the real scala-library: the private
// runtime has no varargs `List.apply`, no `Set` / `Map`, no `Function2`, no
// `ArrowAssoc` and no `scala.runtime.Statics` (which `##` calls), none of
// which has anything to do with `Unit`.

object Main {
  def main(args: Array[String]): Unit = {
    val u1 = ()
    println(().##)
    println(u1.##)
    println(List(()) == List(()))
    println(List((), ()) == List(()))
    println(Some(()) == Some(()))
    println(Option(()) == Some(()))
    println(Set(()).contains(()))
    println(Map("a" -> ()).get("a") == Some(()))
    println((() -> 1) == (() -> 1))
    println(() -> 1)
    // `Unit` on both sides of a function value
    val f: (Unit, Unit) => Boolean = (x, y) => x == y
    println(f((), ()))
    // a `Unit` element compared inside a collection operation
    println(List((), ()).count(_ == ()))
    println(List((), ()).exists(_ != ()))
    println(List(1, 2).map(_ => ()).distinct)
  }
}
