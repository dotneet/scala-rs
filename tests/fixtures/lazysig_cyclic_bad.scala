// Two definitions without a type annotation that need each other's type.
// scalac: `Cyc.scala:2: error: recursive value y needs type` on the `y` of
// `val x = y`.
object A { val x = y; val y = x }
