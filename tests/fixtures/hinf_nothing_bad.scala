// Retracted Nothing, negatives: a definition closes the variable at
// Nothing, so a later line cannot reopen it; an explicit `[Nothing]` and a
// declared `Inv[Nothing]` stay what they say.
object Main {
  class Bld[T] { def +=(t: T): Bld[T] = this }
  object Bld { def newBuilder[T](): Bld[T] = new Bld[T] }
  class Inv[T](val t: T) { def or(u: T): T = u }
  def inv[T](body: => T): Inv[T] = new Inv(body)
  def fail(): Nothing = throw new RuntimeException("no")
  val b = Bld.newBuilder()
  val b2 = b += "a"                          // line 11: Bld[Nothing]
  val y = inv(fail())
  val s: String = y.or("a")                  // line 13: required Nothing
  def d = inv(fail())
  val s2: String = d.or("a")                 // line 15
  val x: Inv[Nothing] = inv(fail())
  val s3: String = x.or("a")                 // line 17
  val w: Inv[String] = inv[Nothing](fail())  // line 18
  def main(args: Array[String]): Unit = ()
}
