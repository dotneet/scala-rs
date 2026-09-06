object Test { def f[T <: Singleton](x: T): T = x; val s = f(new Object) }
