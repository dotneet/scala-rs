// `c.prefix` is a *typed* tree, so a bare name that resolves to a member of an
// enclosing template arrives qualified: `Main.this.macros`, not `macros`. The
// bridge used to send the source `Ident` as it stood, and five corpus tests
// (`macro-term-declared-in-*`, `macro-expand-override`) printed the difference.
//
// A local `val` is *not* a member of a template, so it keeps its bare `Ident`
// -- that is nsc's rule too, and the third line below is what says we did not
// simply qualify everything.
import scala.language.experimental.macros

class Rw3Macros {
  def show: Unit = macro Rw3Impls.showPrefix
}

class Rw3Holder {
  val inner = new Rw3Macros
}

object Main {
  val macros = new Rw3Macros
  val holder = new Rw3Holder

  def main(args: Array[String]): Unit = {
    macros.show
    holder.inner.show
    val local = new Rw3Macros
    local.show
  }
}
