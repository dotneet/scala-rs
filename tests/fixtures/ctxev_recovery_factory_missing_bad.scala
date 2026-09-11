trait Ev[A];class Const[A,B];object Const{def empty[A:Ev,B]:Const[A,B]=new Const[A,B]};object Main{def f[A,B]:Const[A,B]=Const.empty}
