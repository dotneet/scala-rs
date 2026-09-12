// A class defined in a block written directly in a template captures the
// block's locals -- the shape every `object Test extends App { { val x = 2;
// reify { x } } }` of the scala/scala corpus has, and the one the tree
// creator of such a `reify` takes. `L.get` used to read `x` off `this`
// (`NoSuchFieldError: x`); `crate::anon_capture` now treats a template-level
// block local the way `lambda_lift` already did for nested defs.
object Main extends App {
  {
    val x = 2
    class L { def get = x }
    println(new L().get)
    val f = () => x
    println(f())
  };
  {
    var _x = 42
    def x = { val x0 = _x; _x += 1; x0 }
    class M { def get(y: => Int) = y }
    println(new M().get(x))
    println(new M().get(x))
    class N(val k: Int) { def sum = k + x }
    println(new N(1).sum)
  };
  def m(): Int = {
    val y = 7
    class P { def get = y * 2 }
    new P().get
  }
  println(m())
}
