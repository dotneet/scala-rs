object Test { def f[T <: Singleton](x: T): T = x; def g(x: Any): Singleton = f(x) }
