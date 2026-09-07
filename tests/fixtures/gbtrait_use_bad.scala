// The neighbouring case: `new T(arg)` where `T` is a binary trait must still
// be diagnosed. A trait's "constructor" accepts exactly the empty argument
// list; a non-empty one is still `no matching overload`.
object Main {
  val c: GbTraitConstraint = new GbTraitConstraint("x") {
    override def validate(name: String, value: String): Option[String] = None
  }
}
