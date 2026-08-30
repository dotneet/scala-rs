// The dependent member is read off the argument, so it is *checked* too: the
// substitution must not turn a mismatch into an accepted program.
trait Ph { type St }
class P1 extends Ph { type St = Int }

class CS {
  def get[P <: Ph](p: P): Option[p.St] = None
}

object Use {
  val bad: Option[String] = (new CS).get(new P1)
}
