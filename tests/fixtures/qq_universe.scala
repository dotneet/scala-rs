// `import <a reflect universe>._`, and the names it brings into scope.
//
// This is what `import c.universe._` is made of, and what every macro
// implementation writes its body in. The universe's members are declared far
// up its linearisation -- `TermName` on `scala.reflect.api.Names`, `Literal`
// on `Trees`, `Constant` on `Constants`, `termNames` on `StandardNames` --
// and a class read from a jar has its members completed one name at a time.
// Nothing had ever asked for them, so the wildcard import brought in *nothing*
// and every one of these was `not found`. Selecting the same member through
// the path (`u.TermName`) always worked, which is why reified quasiquotes,
// which build `u.TermName(...)` explicitly, never noticed.
//
// The same name is offered in both namespaces (`val TermName` and
// `type TermName`), and they are exposed separately.
//
// Needs scala-reflect.jar on the classpath. Real scalac 2.13.16 prints
// `expected/qq_universe.txt`, and so must we -- these are the same trees.
import scala.reflect.runtime.universe._

object Main {
  // A method-local `import <a value>._` of the same universe. Its prefix is a
  // local, so it may not be used to qualify anything outside this method:
  // doing that emitted a `getfield` for another method's local, which loaded
  // and then died with `NoClassDefFoundError`.
  def viaLocalImport(): String = {
    val u = scala.reflect.runtime.universe
    import u._
    TermName("local").toString + "/" + TypeName("Local").toString
  }

  // Names used as *types*, after the values of the same name were used above.
  def asTypes(): String = {
    val n: TermName = TermName("n")
    val t: TypeName = TypeName("N")
    val c: Constant = Constant(7)
    val lit: Tree = Literal(c)
    s"$n $t $c $lit"
  }

  def main(args: Array[String]): Unit = {
    println(TermName("hi"))
    println(TypeName("T"))
    println(Constant(42))
    println(Literal(Constant(42)))
    println(EmptyTree)
    println(termNames.CONSTRUCTOR)
    println(NoSymbol)
    println(showRaw(Literal(Constant("s"))))
    println(viaLocalImport())
    println(asTypes())
    // A quasiquote reaches the universe the same way, and still builds the
    // tree it built before.
    println(showRaw(q"a.b(1)"))
    // ... and after the method-local import above went out of scope, an
    // unqualified name still resolves through the file-level one.
    println(showRaw(q"c(..${List(q"d", q"e")})"))
    // The bodies `qq_ctx.scala` writes against a macro `Context`, here against
    // a universe that exists at run time, so the trees themselves are checked
    // against scalac's and not only the fact that they compile.
    val x: Tree = q"x"
    println(showRaw(q"scala.List($x, $x)"))
    println(showRaw(q"g(${Literal(Constant(1))})"))
  }
}
