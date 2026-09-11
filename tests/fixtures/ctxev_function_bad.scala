object Main { def id[F[_,_], A, B](x: F[A,B]): F[A,B] = x; def use(f: String => String): String = f("x"); val bad = use(id[Function1,Int,String]((n:Int)=>n.toString)) }
