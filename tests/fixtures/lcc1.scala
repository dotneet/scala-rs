// The reported bug: a `case class` declared inside a method body type-checks
// fine (the typer links a companion symbol in `Typer::ensure_companion`), but
// the backend's `emit_anon_classes` walk over a block's statements only ever
// called `emit_class` for a local `ClassDef` -- never `emit_case_companion`,
// the pass that emits the module class carrying `apply`. `P(1)` therefore
// failed at run time with `NoClassDefFoundError: Main$P$1$`, not at compile
// time.
object Main {
  def main(a: Array[String]): Unit = {
    case class P(n: Int)
    val p = P(1)
    println(p)
    p match {
      case P(x) => println(s"matched $x")
    }
  }
}
