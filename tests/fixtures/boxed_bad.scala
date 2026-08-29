// Conversions real scalac (2.13.16) rejects. Verified one by one against
// `/tmp/scala-2.13.16/bin/scalac`; each line below is an error there too.
object Main {
  // A box only accepts its own primitive: there is no `Long => Integer`.
  val a: java.lang.Integer = 3L
  // ... and no `java.lang.Long => java.lang.Integer` either.
  val b: java.lang.Integer = java.lang.Long.valueOf(3L)
  // Unboxing is just as narrow: `Long2long` gives a `Long`, not an `Int`.
  val c: Int = java.lang.Long.valueOf(3L)
  // A box is not a `String`.
  val d: String = java.lang.Integer.valueOf(3)
  // "Static Java members belong to companion objects in Scala; they are not
  // inherited" — `parseInt` is not reachable through an `Integer` value.
  val e = java.lang.Integer.valueOf(3).parseInt("12")
}
