class B[A](val x:A);class D extends B[Int](7);class E extends B[String]("x");object Main{def f(e:Either[Int,D]):B[Int]=e.fold(n=>new E,identity)}
