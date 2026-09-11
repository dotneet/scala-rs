// Local methods: recursion, mutual recursion, capture of locals and of
// enclosing parameters, local methods used as function values, local
// methods with default arguments, and shadowing.
object Main {
  def outer(n: Int): List[Int] = {
    val k = n * 2
    var hits = 0
    def rec(i: Int): List[Int] = { hits += 1; if (i == 0) Nil else (i + k) :: rec(i - 1) }
    def isEven(i: Int): Boolean = if (i == 0) true else isOdd(i - 1)
    def isOdd(i: Int): Boolean = if (i == 0) false else isEven(i - 1)
    val r = rec(n)
    r ++ List(hits, if (isEven(n)) 1 else 0)
  }
  def withDefaults(): String = {
    def fmt(v: Int, prefix: String = "<", suffix: String = ">"): String = prefix + v + suffix
    List(fmt(1), fmt(2, "["), fmt(3, suffix = "!")).mkString
  }
  def shadow(): String = {
    val x = 1
    def f = x
    locally { val x = 2; def g = x; (f, g).toString }
  }
  def asValue(): List[Int] = { def sq(i: Int) = i * i; List(1, 2, 3).map(sq) }
  def curriedLocal(): Int = { def add(a: Int)(b: Int) = a + b; val add5 = add(5) _; add5(10) }
  def genericLocal(): String = { def dup[A](a: A): (A, A) = (a, a); dup(1).toString + dup("s") }
  def closureOverParam(p: String): () => String = { def mk() = p.toUpperCase; () => mk() }
  def localVarMut(): Int = { var acc = 0; def add(i: Int): Unit = acc += i; (1 to 10).foreach(add); acc }
  def nestedLocal(a: Int): Int = { def l1(b: Int): Int = { def l2(c: Int): Int = a + b + c; l2(b * 10) }; l1(a * 100) }
  def localClassUsesLocalDef(): String = { def helper(s: String) = s * 2; class C { def run = helper("ab") }; new C().run }
  def lazyLocal(): String = { var n = 0; lazy val v = { n += 1; n }; def get = v; s"${get + get} n=$n" }
  def main(args: Array[String]): Unit = {
    println(outer(3)); println(outer(4)); println(withDefaults()); println(shadow()); println(asValue()); println(curriedLocal())
    println(genericLocal()); println(closureOverParam("p")()); println(localVarMut()); println(nestedLocal(1)); println(localClassUsesLocalDef()); println(lazyLocal())
  }
}
