object Test { def f[T <: Singleton](x: T): T = x; var x: Int = 1; val s = f(x) }
