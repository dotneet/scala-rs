// The rest of the quasiquote forms, reified: `tq"..."`, `pq"..."`,
// `cq"..."`, and the `q"..."` shapes `reify_qq.scala` left out -- type
// ascriptions, eta expansion, blocks, `new`, `match`, function literals.
//
// `docs/macros.md` §7.7. Every line is compared with real scalac 2.13.16,
// which prints `expected/qr_forms.txt`: `showRaw` means the comparison is of
// the *trees*, not of anything that merely typechecks. The shapes were read
// off nsc with `-Ymacro-debug-lite`, which prints what its own quasiquote
// macro expands to.
//
// Needs scala-reflect.jar on the classpath; `import <universe>._` is what
// puts `q` in scope in the first place.
import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    val x: Tree = q"x"
    val t: Tree = tq"Int"
    val p: Tree = pq"h"
    val n: TermName = TermName("nm")
    val tn: TypeName = TypeName("NM")
    val xs: List[Tree] = List(q"a", q"b")

    // --- tq"...": types -------------------------------------------------
    println(showRaw(tq"Int"))
    println(showRaw(tq"_root_.scala.Int"))
    println(showRaw(tq"Option[$t]"))
    // `A => B` is `_root_.scala.FunctionN[...]`, a *written* `Function1` is
    // not. The parser folds both into one node, so reification reads the
    // source text back to tell them apart.
    println(showRaw(tq"$t => $t"))
    println(showRaw(tq"($t, $t) => $t"))
    println(showRaw(tq"() => $t"))
    println(showRaw(tq"Function1[$t, $t]"))
    println(showRaw(tq"(Int, String)"))
    println(showRaw(tq"a.b.C"))
    println(showRaw(tq"_root_.scala.collection.immutable.Nil.type"))
    println(showRaw(tq"A#B"))
    println(showRaw(tq"$t#$tn"))
    println(showRaw(tq"A with B"))
    println(showRaw(tq"$t"))

    // --- q"...": the shapes slick's `mapToImpl` is written in ------------
    println(showRaw(q"$x : Int"))
    println(showRaw(q"($x.tupled) : ($t => $t)"))
    println(showRaw(q"($x.apply _) : ($t => $t)"))
    println(showRaw(q"(($x.apply _).tupled) : ($t => $t)"))
    println(showRaw(q"$x.asInstanceOf[_root_.scala.Any => _root_.scala.Any]"))

    // --- q"...": blocks and definitions ---------------------------------
    println(showRaw(q"{ val v = $x; v }"))
    println(showRaw(q"{ val v: $t = $x; v }"))
    println(showRaw(q"{ $x; $x }"))
    println(showRaw(q"{ ..$xs }"))
    println(showRaw(q"val v = $x"))

    // --- q"...": `new`, `match`, functions, and the rest -----------------
    println(showRaw(q"new Foo"))
    println(showRaw(q"new Foo(1)"))
    println(showRaw(q"new Foo[$t](..$xs)"))
    println(showRaw(q"new Foo(1)(2)"))
    println(showRaw(q"a match { case b => c }"))
    println(showRaw(q"{ case v => $x }"))
    println(showRaw(q"(y: Int) => y"))
    println(showRaw(q"this"))
    println(showRaw(q"Foo.this"))
    println(showRaw(q"a.b = c"))
    println(showRaw(q"if (a) b else c"))
    println(showRaw(q"f[$t](1)"))
    // An operator name is encoded the way nsc's parser encodes it.
    println(showRaw(q"a.+(b)"))
    // A hole may stand for the *name* of a selection.
    println(showRaw(q"$x.$n"))
    println(showRaw(q"$x.$n(..$xs)"))

    // --- pq"...": patterns ----------------------------------------------
    println(showRaw(pq"_"))
    // A lower-case name is a variable pattern (`Bind`), an upper-case one a
    // stable identifier.
    println(showRaw(pq"h"))
    println(showRaw(pq"Foo"))
    println(showRaw(pq"_root_.scala.None"))
    println(showRaw(pq"Foo(a, b)"))
    println(showRaw(pq"a.b.Foo(c)"))
    println(showRaw(pq"y @ $p"))
    println(showRaw(pq"a | b"))
    println(showRaw(pq"_: $t"))
    println(showRaw(pq"1"))

    // --- cq"...": one case clause, written without its `case` ------------
    println(showRaw(cq"$p => $x"))
    println(showRaw(cq"Foo(a) if a => a"))

    // --- the tree factories that are overload sets ------------------------
    //
    // `val Ident: IdentExtractor` sits next to `def Ident(name: String)`, and
    // the same for `Bind`, `This` and `New`, so `Ident(TermName("x"))`
    // matches no alternative of the set and is `Ident.apply(...)`. Without
    // the `apply` insertion reaching overload sets, none of these compiled --
    // slick's `TableQuery` macro implementation is written entirely in them.
    println(showRaw(Ident(TermName("tag"))))
    println(showRaw(Bind(TermName("b"), Ident(termNames.WILDCARD))))
    println(showRaw(This(TypeName("Foo"))))
    println(showRaw(New(Ident(TypeName("Foo")))))
    println(showRaw(Apply(Select(New(Ident(TypeName("Foo"))), termNames.CONSTRUCTOR),
      List(Ident(TermName("tag"))))))
  }
}
