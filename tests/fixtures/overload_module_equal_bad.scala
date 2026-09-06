class X { def f(n: Int) = "method"; object f { def apply(n: Int) = "object" } }
object Main extends App { println(new X().f(1)) }
