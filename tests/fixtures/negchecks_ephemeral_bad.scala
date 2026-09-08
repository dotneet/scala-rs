trait U extends Any {

  var v = 1

  { println("side effect") }

  val w: Int

  object Nested

  def ok: Int = 1

  type T = Int

  class AlsoOk { def n = 1 }
}

class V(val s: String) extends AnyVal {
  object Companionless
  trait Inner { def q: String }
  class Held(val q: String)
  def deep: Any = {
    object Buried
    class AlsoBuried
    Buried
  }
  def fine: String = s
  type Alias = String
}
