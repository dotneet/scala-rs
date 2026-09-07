// `new T()` where `T` is a trait read from a class file, not from source.
// `GbTraitConstraint` is compiled separately (see `gbtrait_lib.scala`) and
// handed to this compile only as a `-cp` class file, exactly like gitbucket's
// `org.scalatra.forms.Constraint`.
object Main {
  val c: GbTraitConstraint = new GbTraitConstraint() {
    override def validate(name: String, value: String): Option[String] =
      Some(name + "=" + value)
  }
  def main(args: Array[String]): Unit = println(c.validate("a", "b"))
}
