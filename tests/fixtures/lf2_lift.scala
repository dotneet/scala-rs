// `Liftable`: quasiquote holes whose argument is not a `Tree`.
//
// `docs/macros.md` §7.8. nsc infers an implicit `Liftable[T]` for such a hole
// and splices `Liftable.liftX[T](arg)` (`scala/reflect/api/StandardLiftables
// .scala`); scala-rs picks the standard instance from the argument's type and
// builds the same tree that instance builds. This file is what says the trees
// really are the same: every line prints `showRaw` (and `show` where the tree
// is a `TypeTree`, whose `showRaw` hides the type it carries), and real
// scalac 2.13.16 prints `expected/lf2_lift.txt` too.
//
// The `WeakTypeTag` and `Expr` instances -- the two slick's
// `ShapedValue.mapToImpl` needs -- are in `lf2_ctx.scala` instead: a tag can
// only be got from a materialiser or a macro's implicit parameter, and this
// file has neither.
//
// Needs scala-reflect.jar on the classpath; `import <universe>._` is what
// puts `q` in scope in the first place.
import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    val x: Tree = q"x"
    val n: TermName = TermName("nm")
    val tn: TypeName = TypeName("NM")
    val i: Int = 42
    val l: Long = 7L
    val s: String = "hi"
    val b: Boolean = true
    val c: Char = 'q'
    val d: Double = 2.5
    val k: Constant = Constant(3)
    val t = scala.reflect.runtime.universe.definitions.IntTpe
    val sym = scala.reflect.runtime.universe.definitions.ListModule
    val ns: List[TermName] = List(TermName("a"), TermName("b"))
    val vs: List[Int] = List(1, 2)
    val ts: Vector[Tree] = Vector(q"p", q"r")

    // --- literals: `liftInt` and its siblings ---------------------------
    println(showRaw(q"f($i)"))
    println(showRaw(q"f($l)"))
    println(showRaw(q"f($s)"))
    println(showRaw(q"f($b)"))
    println(showRaw(q"f($c)"))
    println(showRaw(q"f($d)"))
    println(showRaw(q"f($k)"))

    // --- names: spliced into the identifier the hole stands for ---------
    println(showRaw(q"$n"))
    println(showRaw(q"f($n)"))
    println(showRaw(q"$n.foo"))
    println(showRaw(tq"$tn"))
    println(showRaw(pq"$n"))
    println(showRaw(q"val $n = $x"))
    println(showRaw(q"new $tn(1)"))

    // --- types: `TypeTree(tpe)` -----------------------------------------
    println(showRaw(q"f($t)"))
    println(show(q"f($t)"))
    println(showRaw(tq"$t"))
    println(show(tq"$t"))
    println(show(tq"($t) => $t"))
    println(show(q"new $t(1)"))
    println(show(q"x.asInstanceOf[$t]"))

    // --- symbols: `mkRefTree`, not a `Liftable` at all -------------------
    println(showRaw(q"f($sym)"))
    println(show(q"f($sym)"))
    println(show(q"$sym.apply(1)"))

    // --- `..$`: the elements are lifted one by one ----------------------
    println(showRaw(q"f(..$ns)"))
    println(showRaw(q"f(..$vs)"))
    println(showRaw(q"f(..$ts)"))
  }
}
