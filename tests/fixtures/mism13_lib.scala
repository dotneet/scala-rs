// `scala.<:<` used as an implicit *view*.
//
// nsc asks whether an implicit's type conforms to `From => To`, so a value of
// any class that inherits `Function1` is a conversion --
// `sealed abstract class <:<[-From, +To] extends (From => To)`. slick's
// `def flatten[QO](implicit ev: P <:< Rep[Option[QO]]) = flatMap[QO](identity(_))`
// (`lifted/ExtensionMethods.scala`) leans on nothing else.
//
// `<:<` lives only in the real `scala-library` ABI, so this fixture is
// jar-only; `mism13_lib_without_library_is_error` pins the diagnostic the
// private runtime gives instead.

class Rp[T](val s: String)

class Ext[P](val r: P) {
  def flatMap[QO](f: P => Rp[Option[QO]]): Rp[Option[QO]] = f(r)
  // The body is a `P`; only `ev` can make it a `Rp[Option[QO]]`.
  def flatten[QO](implicit ev: P <:< Rp[Option[QO]]): Rp[Option[QO]] =
    flatMap[QO](identity(_))
  def direct[QO](implicit ev: P <:< Rp[Option[QO]]): Rp[Option[QO]] = r
}

object Main {
  def main(args: Array[String]): Unit = {
    val e = new Ext[Rp[Option[Int]]](new Rp[Option[Int]]("inner"))
    println(e.flatten[Int].s)
    println(e.direct[Int].s)
    // An ordinary function-typed implicit still works the same way.
    implicit val conv: Int => String = i => "i" + i
    val f: String = 41 + 1
    println(f)
  }
}
