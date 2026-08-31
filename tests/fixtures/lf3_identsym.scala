// `agent/liftable`'s residual: `u.Ident(sym: Symbol)`, the overload of the
// tree factory `val Ident: IdentExtractor` sits next to but is not part of --
// it is a separate convenience method `def Ident(sym: Symbol): Ident`
// declared directly on `scala.reflect.internal.Trees` (verified with `javap`
// against scala-reflect.jar 2.13.16: `scala/reflect/api/Trees.class` declares
// `abstract Trees$IdentApi Ident(Symbols$SymbolApi)` right next to the
// extractor's own `apply(Name)`). slick's `TableQueryMacroImpl.apply` is
// written in exactly this line:
//
//     Ident(typeOf[Tag].typeSymbol)
//
// Before this fixture, `PickleSupply::erased_param_desc`
// (`crates/typer/src/pickle_supply.rs`) had no case for `Type::TypeMember` --
// an abstract type member like `Symbol`, reached from the abstract
// `scala.reflect.api.Trees`/`Universe` API rather than the concrete
// `JavaUniverse` a macro only gets at actual expansion time -- and fell
// through to `None`, the "any reference slot" wildcard. `Ident(String)` and
// `Ident(Symbol)` are both one reference parameter at that abstract level, so
// the wildcard could not tell the two classfile methods apart
// ("no unambiguous erased descriptor") and the `Symbol` overload was silently
// never installed: `Ident(sym)` reported `no matching overload for <overload
// Trees$IdentExtractor | (String)Trees.Ident> with arguments (Symbol)`, one
// candidate short. nsc itself erases an abstract type to its own upper bound
// (`SymbolApi` here, `Object` with none), which is what the fix now does too.
//
// `typeOf[Tag]` itself needs `TypeTag` materialization
// (`docs/macros.md` §7.8, `lf2_ctx.scala`'s own note), a separate
// unimplemented compiler-internal macro -- so this fixture reaches a
// `Symbol` the way `lf2_ctx.scala`'s `symbols` already does, through the
// macro's own implicit tag parameter (`weakTypeOf[E].typeSymbol`), which
// needs no materialization and isolates the overload-supply gap on its own.
//
// Compile-only, and compiled by real scalac 2.13.16 too, the same way
// `lf2_ctx.scala` is: *calling* a macro needs the JVM bridge
// (`docs/macros.md` §2.2), which is not built.
import scala.reflect.macros.blackbox.Context

object Lf3IdentSym {
  // The slick shape itself: an `Ident` built directly from a type's own
  // symbol, exactly where `TableQueryMacroImpl.apply` builds the `Tag`
  // parameter's `Ident`.
  def tagIdent[E](c: Context)(implicit e: c.WeakTypeTag[E]): c.Tree = {
    import c.universe._
    Ident(weakTypeOf[E].typeSymbol)
  }

  // `Symbol` reached other ways: `c.internal.enclosingOwner`, and the same
  // `Apply(Select(New(...), termNames.CONSTRUCTOR), ...)` shape
  // `TableQueryMacroImpl.apply`'s very next line builds, reading an `Ident`
  // back off a constructed tree's own type.
  def enclosing(c: Context): c.Tree = {
    import c.universe._
    Ident(c.internal.enclosingOwner)
  }

  // `Ident` used as the type position of a synthesized parameter's `tpt`,
  // and `Apply(Select(New(...), termNames.CONSTRUCTOR), List(Ident(...)))`
  // applied to it -- the two lines around `TableQueryMacroImpl.apply`'s own
  // `Ident(typeOf[Tag].typeSymbol)`. (`Function(params, body)`, the tree
  // factory around this shape in the real macro, hits an unrelated,
  // pre-existing gap -- `value apply is not a member of Function$` -- so
  // this checks the `Ident` usage on its own instead of through it.)
  def tableQueryShape[E](c: Context)(implicit e: c.WeakTypeTag[E]): c.Tree = {
    import c.universe._
    val tpt: Tree = Ident(weakTypeOf[E].typeSymbol)
    val param: Tree = ValDef(Modifiers(Flag.PARAM), TermName("tag"), tpt, EmptyTree)
    val ctorCall: Tree = Apply(
      Select(New(TypeTree(weakTypeOf[E])), termNames.CONSTRUCTOR),
      List(Ident(TermName("tag")))
    )
    q"{ ..${List(param, ctorCall)} }"
  }
}
