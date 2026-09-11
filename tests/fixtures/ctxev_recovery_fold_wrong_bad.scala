object Main{def f(e:Either[Int,String]):Int=e.fold(identity,identity)}
