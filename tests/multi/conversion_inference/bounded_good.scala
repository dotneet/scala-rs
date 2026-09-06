class From; class To[T <: CharSequence] { def foo(x:T):T=x }; object Test { implicit def conv[T <: CharSequence](x:From):To[T]= ???; val from=new From; val x=from.foo("hi") }
