// Compiled by scalac and packed into a jar by `crates/cli/tests/mixcg.rs`:
// a trait read from a jar reaches scala-rs through its pickle, which carries
// neither its `m$` statics nor its `$$super$` accessors.
package mixcglib

trait Silent extends Throwable {
  override def fillInStackTrace(): Throwable = if (quiet) this else super.fillInStackTrace()
  def quiet: Boolean = true
}

class Base { def label: String = "base" }
trait Loud extends Base { override def label: String = "loud(" + super.label + ")" }

abstract class Shape { def sides: Int }
trait Square extends Shape { def sides: Int = 4 }
