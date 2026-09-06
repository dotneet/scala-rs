class X { def f(n: Int) = "method"; object f { def apply(n: Any) = "object" } }
object Main extends App { println(new X().f(1)) }
