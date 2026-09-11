object Main{def choose[A](f:A=>A,g:()=>A):A=f(g());val f:String=>String=s=>s;val x=choose(f,()=>3)}
