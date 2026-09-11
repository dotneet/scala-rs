// A parent constructor's arguments are typed outside the template: its own
// members, terms and types alike, are not in scope there.
class P(val p: Int)
class Q(val q: Any)
object O {
  class D extends P(a) { def a = 1 }
  class F(y: Int) extends Q(new Inner(y)) { class Inner(v: Int) }
  class G extends Q(() => b) { def b = 2 }
}
