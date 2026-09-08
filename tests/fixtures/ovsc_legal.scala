// The over-reach guard for the two new rejections in `agent/overscore`.
//
// Every `new` here is legal Scala that real scalac 2.13.16 accepts, and every
// one of them sits next to something the new checks refuse: a bound that is
// met, a bound stated in another parameter, a bound on a higher-kinded
// parameter (whose bound is written in that parameter's *own* arguments and so
// is not a proper type to compare against -- getting this wrong cost one new
// `scala/collection/Iterable.scala` error the first time), a bound behind a
// `with` type, an existential argument, and a `new C[K, V]` written from
// inside `C` itself.
//
// The point of the file is that the **pre-fix binary compiles it and prints
// exactly the same thing**. If a future change to `check_class_tparam_bounds`
// or to `apply_types`' arity check starts rejecting one of these, this is
// where it shows up.
package ovsc

class Bounded2b[K <: AnyRef, V](val k: K, val v: V)
class Cellb[K, V](val k: K, val v: V)
class Pair[A, B <: A](val a: A, val b: B)
class Ser[T <: AnyRef with java.io.Serializable](val t: T)
trait Boxy[X] { def get: X }
class OneBox[X](val get: X) extends Boxy[X]
class Holder[F[X] <: Boxy[X], A](val f: F[A])
class Self[K <: AnyRef, V](val k: K, val v: V) {
  // The `agent/triemapjava` shape: written arguments that *are* the
  // instantiated class's own parameters. Their bounds are the declared ones,
  // so they conform by construction.
  def again: Self[K, V] = new Self[K, V](k, v)
}

object Legal {
  def main(args: Array[String]): Unit = {
    println(new Bounded2b[String, Int]("k", 1).k)
    println(new Cellb[String, Int]("c", 2).v)
    println(new Pair[AnyRef, String]("p", "q").b)
    println(new Ser[String]("s").t)
    // The `new` is what this file is about. Reading `h.f.get` back would run
    // into a *separate*, pre-existing codegen defect -- an `F[A]` field erases
    // to `F`'s bound and the read is not cast to the applied constructor, so
    // the JVM verifier rejects `getfield get` with a `Boxy` on the stack. The
    // pre-fix binary emits the identical bad method; it has nothing to do with
    // this slice and is written up in `docs/scala-library.md`.
    val h = new Holder[OneBox, Int](new OneBox(3))
    println("holder:" + (h != null))
    println(new Self[String, Int]("self", 5).again.k)
    // An existential argument says nothing about the bound and must not be
    // second-guessed.
    val e: Cellb[Boxy[_], Int] = new Cellb[Boxy[_], Int](new OneBox(6), 7)
    println(e.v)
  }
}
