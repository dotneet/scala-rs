// Macro implementations whose expansions contain `Function` and `ValDef`.
// `docs/macros.md` §7.13. Compiled on its own so `sd_use.scala` can expand
// against it, the split nsc requires (§1.3).
//
// This is the shape of slick's `TableQueryMacroImpl.apply`, which is what
// stage D is aiming at:
//
//   val cons = c.Expr[Tag => E](Function(
//     List(ValDef(Modifiers(Flag.PARAM), TermName("tag"),
//                 Ident(typeOf[Tag].typeSymbol), EmptyTree)),
//     Apply(Select(New(TypeTree(e.tpe)), termNames.CONSTRUCTOR),
//           List(Ident(TermName("tag"))))))
//
// Every piece of that tree is written out here: the function literal, the
// parameter `ValDef` with its `Modifiers(Flag.PARAM)`, the type `Ident` built
// from a symbol, the `New` over a `TypeTree` of the tag's type, and a
// reference from the body back to the parameter -- a name with no symbol the
// bridge could carry, so it has to be re-bound at the call site.
import scala.reflect.macros.blackbox.Context

/** A stand-in for slick's `Tag`. */
class SdTag(val name: String)

// The rows the macro builds. They live *here*, not next to the call sites,
// because the tag for `E` is a `staticClass("SdCoffees")` the engine resolves
// in the macro classpath -- which holds what an *earlier* run compiled. A row
// class defined in the same file as the call would not be there yet, and the
// expansion says so rather than guessing (`sd_gaps_bad.scala` pins it).
class SdCoffees(tag: SdTag) {
  def label: String = "coffees@" + tag.name
}

class SdTeas(tag: SdTag) {
  def label: String = "teas@" + tag.name
}

class SdQuery[E](cons: SdTag => E) {
  def head: E = cons(new SdTag("t0"))
}

object SdQuery {
  def apply[E](cons: SdTag => E): SdQuery[E] = new SdQuery[E](cons)
}

object SdImpl {
  // slick's `TableQueryMacroImpl.apply`, minus the `reify` at the end: the
  // expansion is `SdQuery.apply[E](tag => new E(tag))`, and the function
  // literal is built node by node.
  def queryImpl[E](c: Context)(implicit e: c.WeakTypeTag[E]): c.Expr[SdQuery[E]] = {
    import c.universe._
    val cons = Function(
      List(
        ValDef(
          Modifiers(Flag.PARAM),
          TermName("tag"),
          Ident(typeOf[SdTag].typeSymbol),
          EmptyTree
        )
      ),
      Apply(
        Select(New(TypeTree(e.tpe)), termNames.CONSTRUCTOR),
        List(Ident(TermName("tag")))
      )
    )
    c.Expr[SdQuery[E]](
      Apply(Select(Ident(TermName("SdQuery")), TermName("apply")), List(cons))
    )
  }

  // A bare function literal as the whole expansion: two parameters, and a
  // body that mentions both of them.
  def adderImpl(c: Context): c.Expr[(Int, Int) => Int] = {
    import c.universe._
    c.Expr[(Int, Int) => Int](
      Function(
        List(
          ValDef(Modifiers(Flag.PARAM), TermName("p"), TypeTree(typeOf[Int]), EmptyTree),
          ValDef(Modifiers(Flag.PARAM), TermName("q"), TypeTree(typeOf[Int]), EmptyTree)
        ),
        Apply(Select(Ident(TermName("p")), TermName("$plus")), List(Ident(TermName("q"))))
      )
    )
  }

  // A modifier the bridge has no name for. Every flag travels by name and
  // the table covers what a `val` in an expression can carry; `DEFERRED` --
  // an abstract member -- is not on it, so the expansion is refused rather
  // than rebuilt without it. (Real scalac rejects this expansion too, for its
  // own reason: a block cannot declare an abstract value.)
  def deferredImpl(c: Context): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](
      Block(
        List(ValDef(Modifiers(Flag.DEFERRED), TermName("v"), TypeTree(typeOf[Int]), EmptyTree)),
        Literal(Constant(1))
      )
    )
  }

  // A `ValDef` that is not a parameter: a local value in a block, with no
  // modifiers at all. The argument is evaluated once and used twice, which is
  // what a `val` in an expansion is normally for.
  def twiceImpl(c: Context)(x: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](
      Block(
        List(ValDef(NoMods, TermName("v"), TypeTree(typeOf[Int]), x.tree)),
        Apply(Select(Ident(TermName("v")), TermName("$plus")), List(Ident(TermName("v"))))
      )
    )
  }
}
