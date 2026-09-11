// Three typer repairs every slick `mapTo` expansion depends on, each shown
// here without the macro. Real scalac 2.13.16 compiles and runs this file
// and `crates/cli/tests/gbmac.rs` requires scala-rs to print the same.
//
// 1. An inherited alias to an abstract projection: `type Reader = M#Reader`
//    seen from a subclass that fixes `M` is `IntDomain#Reader`, which is
//    `Int`. scala-rs returned the alias unsubstituted (`M#Reader`), so
//    `r: Reader` and `next[Int].read`'s parameter never met.
// 2. A case class's `copy` read from a jar: nsc pickles it `SYNTHETIC`
//    without `CASE`, so it was dropped with the synthetic plumbing and the
//    class file's `copy(x$0, x$1, ...)` stood in -- `DumpInfo("a").copy(name
//    = "b")` was "unknown parameter name: name". The expansion's fast-path
//    converter writes `super.getDumpInfo.copy(name = ...)`.
// 3. A type member a jar class passes to a subclass: slick's `ResultConverter`
//    declares `protected[this] type Reader = M#Reader`, which no bytecode
//    records, and a bare `Reader` in a subclass body was "not found".
import slick.relational._
import slick.util.DumpInfo

trait Domain { type Reader }
trait IntDomain extends Domain { type Reader = Int }
abstract class Conv[M <: Domain, T] {
  type Reader = M#Reader
  def read(pr: Reader): T
}
abstract class Fast[M <: Domain, T] extends Conv[M, T] {
  def next[C]: Conv[M, C] = new Conv[M, C] {
    def read(pr: Reader): C = pr.asInstanceOf[C]
  }
}
class Local extends Fast[IntDomain, String] {
  val a = next[Int]
  override def read(r: Reader): String = "read " + (a.read(r) + 1)
}
class Direct extends Conv[IntDomain, String] {
  def twice(r: Reader): Int = r * 2
  override def read(r: Reader): String = "direct " + twice(r)
}

class Leaf(tm: TypeMappingResultConverter[ResultConverterDomain, String, _])
    extends SimpleFastPathResultConverter[ResultConverterDomain, String](tm) {
  override def read(r: Reader): String = "leaf"
  override def set(value: String, pp: Writer): Unit = ()
  override def update(value: String, pr: Updater): Unit = ()
  override def getDumpInfo = super.getDumpInfo.copy(name = "renamed", mainInfo = "leaf")
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Local().read(41))
    println(new Direct().read(21))
    val anon = new Fast[IntDomain, String] {
      val b = next[Int]
      override def read(r: Reader): String = "anon " + b.read(r)
    }
    println(anon.read(7))
    val d = DumpInfo("a", "main")
    println(d.copy(name = "b"))
    println(d.copy(mainInfo = "m2", attrInfo = "x"))
    val inner = new ResultConverter[ResultConverterDomain, Any] {
      def read(pr: Reader): Any = null
      def update(value: Any, pr: Updater): Unit = ()
      def set(value: Any, pp: Writer): Unit = ()
      def width = 1
    }
    val tm = TypeMappingResultConverter[ResultConverterDomain, String, Product](
      ProductResultConverter[ResultConverterDomain, Product](inner),
      (s: String) => Tuple1(s),
      (p: Product) => p.productElement(0).toString)
    println(new Leaf(tm).getDumpInfo.copy(children = Vector()))
  }
}
