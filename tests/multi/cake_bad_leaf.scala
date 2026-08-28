package cake.bad

import cake.badbase.Base

/** Resolving inherited names across files must stay honest: a name that is
  * not in the linearization is still an error, even though the parent chain
  * lives in a file that comes later on the command line. */
object Leaf extends Base {
  def ok(p: Present[Int]): String = p.n
  def missing(m: Missing[Int]): String = m.toString
  def notMixedIn(d: Detached[Int]): String = d.toString
}
