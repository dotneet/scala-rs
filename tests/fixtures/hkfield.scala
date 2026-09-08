// `agent/hkfield`: reading a member off a value whose declared type erases to
// a *bound* rather than to `Object`.
//
// Every read below sits at a type the typer has instantiated more precisely
// than the JVM descriptor says. `val f: F[A]` in `Holder` has the descriptor
// `Lhkf/Boxy;` -- `F`'s bound -- while `h.f` at `Holder[OneBox, Int]` is an
// `OneBox[Int]`, so a `checkcast hkf/OneBox` is owed before `tag` is read off
// it. scalac 2.13.16 emits exactly that; without it the JVM verifier rejects
// the enclosing method outright (`Bad type on operand stack`).
//
// `tag` and `wtag` are declared on the *concrete* class and not on the bound,
// which is what makes the missing cast reach the verifier: a member found on
// the bound itself (`get`, `peel`) is read through the bound's own descriptor
// and needs nothing.
package hkf

trait Boxy[X] { def get: X }
class OneBox[X](val get: X) extends Boxy[X] { val tag: String = "one" }

// (1) The reported defect: a field at a higher-kinded parameter.
class Holder[F[X] <: Boxy[X], A](val f: F[A])

// (2) The same root, first order: a field at a *bounded* type parameter.
// `T <: Boxy[Int]` erases to `Boxy` for exactly the same reason.
class BHolder[T <: Boxy[Int]](val f: T)

// (3) The shape that already worked -- an unbounded parameter erases to
// `Object`, which was the only case the load path used to cast for. Kept so a
// change here cannot silently drop its cast.
class OHolder[T](val f: T)

// (4) A *method* returning `F[A]`.
class MHolder[F[X] <: Boxy[X], A](g: F[A]) { def m: F[A] = g }

// (5) `F[A]` reached through a pattern: a type test, then the field read.
class PBox[F[X] <: Boxy[X], A](val f: F[A])

// (5b) The stronger pattern form -- a case-class extractor that *binds* the
// `F[A]`. This is a second site of the same root and is not reached by fixing
// the field-read path: the match lowering reads the constructor field itself
// (`gen_ctor_fields_pattern`) and had its own `Object`-only narrowing test, so
// the binder held a `Boxy` while the typer had given it `OneBox[Int]`.
case class CBox[F[X] <: Boxy[X], A](f: F[A], n: Int)

// (6) `F`'s own bound is higher-kinded.
trait Wrap[G[_]] { def peel: G[Int] }
class WOne[G[_]](val peel: G[Int]) extends Wrap[G] { val wtag: String = "wone" }
class WHolder[F[G[_]] <: Wrap[G], G[_]](val w: F[G])

// The over-reach guard: a field whose declared erasure already *is* the class
// the tree wants must get no cast at all -- an unnecessary `checkcast` is a
// divergence from scalac in the other direction.
class Plain(val c: OneBox[Int])

object Main {
  // (7) `F[A]` as a parameter. `get` is the bound's own member, so scalac
  // emits no cast here and neither may we.
  def viaParam[F[X] <: Boxy[X], A](p: F[A]): Int = p.get.toString.length

  def main(args: Array[String]): Unit = {
    val h = new Holder[OneBox, Int](new OneBox(1))
    println(h.f.tag)
    println(h.f.get)
    // Wanted only at the bound. scalac emits no cast here, because its erasure
    // adapts to the *expected* type; this compiler's load path casts to the
    // tree's own type and so emits a (harmless, always-succeeding)
    // `checkcast OneBox`. That is not new here -- the sibling rule on the
    // method-result path (`maybe_unbox_erased_result`) has done exactly the
    // same at `val mb: Boxy[Int] = mholder.m` since long before this slice, and
    // so does the `Object` arm of this very function at
    // `val ob: Boxy[Int] = oholder.f`. Kept in the fixture so the difference is
    // recorded rather than discovered later.
    val hb: Boxy[Int] = h.f
    println(hb.get)

    println(new BHolder[OneBox[Int]](new OneBox(2)).f.tag)
    println(new OHolder[OneBox[Int]](new OneBox(3)).f.tag)
    println(new MHolder[OneBox, Int](new OneBox(4)).m.tag)
    println(viaParam[OneBox, Int](new OneBox(5)))

    val pb = new PBox[OneBox, Int](new OneBox(6))
    pb match {
      case p: PBox[OneBox, Int] => println(p.f.tag)
    }

    val cb = CBox[OneBox, Int](new OneBox(9), 3)
    cb match {
      case CBox(b, n) => println(b.tag + n)
    }
    val CBox(b2, n2) = cb
    println(b2.tag + n2)

    val wh = new WHolder[WOne, OneBox](new WOne[OneBox](new OneBox(7)))
    println(wh.w.wtag)
    println(wh.w.peel.get)

    println(new Plain(new OneBox(8)).c.tag)
  }
}
