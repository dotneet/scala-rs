class X { def f(n: Int) = "method"; def f[A](n: A) = "generic" }
object Main extends App { println(new X().f(1)) }
