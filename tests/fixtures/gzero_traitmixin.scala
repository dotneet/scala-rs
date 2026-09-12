// A trait that arrived as a **class file** and defines a `val`, a `var` and
// nested `object`s. The class mixing it in owes an implementation of every one
// of those accessors; without the module half, the first read from the trait's
// own code was an `AbstractMethodError`, and without the typer half, the
// program did not compile at all ("Missing implementations for ... members").
object GzeroTraitMixin {
  object O extends gzerolib.HasNested
  class C extends gzerolib.HasStuff
  class D extends gzerolib.HasNested {
    override val greeting: String = "hi"
  }

  def main(args: Array[String]): Unit = {
    println(O.deep)
    val c = new C
    c.seen = 7
    println(c.show)
    println(new D().deep)
    // Read from the trait's own code, which is what the mixin accessor is
    // for. Naming `O.Inner` from *outside* is a separate gap: a jar trait's
    // nested module class arrives as an unresolved name, so the selection's
    // descriptor comes out without its package.
  }
}
