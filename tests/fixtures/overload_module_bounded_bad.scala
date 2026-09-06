class X { def f(n: 1) = "method"; def f[A <: 1](n: A) = "generic" }
object Main extends App { println(new X().f(1)) }
