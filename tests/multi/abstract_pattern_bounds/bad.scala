object Bad { type Foo[A] <: A; def test(x: Foo[String]): Int = x match { case i: Int => i } }
