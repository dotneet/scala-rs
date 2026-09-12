// `flatMap`'s function must return an `IterableOnce` (an `Option` for
// `Option.flatMap`). The body is typed against `Wildcard`, because the
// declared result `IterableOnce[B]` is what the body determines, and nothing
// checked it afterwards -- so these compiled (agent/erascg).
object Test {
  val a = Seq(1).flatMap(x => x)
  val b = List(1).flatMap(x => x + 1)
  val c = Option(1).flatMap(x => x)
  val d = Iterator(1).flatMap(x => x)
}
