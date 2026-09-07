// The "library": a trait, compiled on its own so the consumer only ever sees
// it as a class file (and, in library-ABI mode, its ScalaSignature pickle) --
// never as source. Mirrors org.scalatra.forms.Constraint from gitbucket's
// dependency closure, which compiles to a JVM interface with no `<init>` at
// all (`javap -p` confirms this for the real jar).
trait GbTraitConstraint {
  def validate(name: String, value: String): Option[String]
}
