trait AliasValue { def n: Int }
class AliasOwner {
  type Event = AliasValue
  val e: Event = new AliasValue { def n: Int = 7 }
}
object Main {
  def main(args: Array[String]): Unit = {
    { val o = new AliasOwner; import o._; def f(x: Event): Int = x.n; println(f(e)) };
    { val o: AliasOwner = new AliasOwner; import o._; def f(x: Event): Int = x.n; println(f(e)) };
    { val o = new AliasOwner; import o.{Event, e}; def f(x: Event): Int = x.n; println(f(e)) };
    { val o = new AliasOwner; import o._; println(f(e)); def f(x: Event): Int = x.n };
  }
}
