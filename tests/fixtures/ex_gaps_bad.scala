// Receiverless macro calls remain unsupported. The written `new` receiver
// below is supported and separately executed with a side-effect counter in
// macrotransportbatch; it must not obscure the receiverless diagnostic.
import scala.language.experimental.macros

class ExGap(val tag: String) {
  def label: String = macro ExImpl.tagImpl

  // Called with no receiver at all: nsc synthesises `This(ExGap)` for the
  // prefix, which is a tree the bridge does not build yet.
  def relabel: String = label
}

object ExGapMain {
  def main(args: Array[String]): Unit = {
    // Written constructor receivers survive the structural transport.
    println(new ExGap("x").label)
  }
}
