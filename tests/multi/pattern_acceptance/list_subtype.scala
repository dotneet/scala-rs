class C; class D extends C; object Test { def f(xs:Seq[D]):Int=xs match {case _:List[C] => 1; case _ => 0} }
