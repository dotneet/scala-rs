trait Ev[A];class Const[A,B];object Const{def empty[A:Ev,B]:Const[A,B]=new Const[A,B]};object Main{def f[A,B](implicit e:Ev[String]):Const[A,B]=Const.empty}
