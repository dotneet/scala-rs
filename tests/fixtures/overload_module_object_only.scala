class X { def f(n: String) = "method"; object f { def apply(n: Int) = "object" } }
object Main extends App { println(new X().f(1)) }
