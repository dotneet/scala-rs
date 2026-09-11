class Box[F[_],A](val x:F[A]);object Main{val bad:Box[Option,Int]=new Box[Option,String](Some("bad"))}
