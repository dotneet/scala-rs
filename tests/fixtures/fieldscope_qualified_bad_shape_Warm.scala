package aaa
object Warm { private[aaa] def secret(x:Int):Int=x+1;val f:Int=>Int=(x:Int)=>aaa.Warm.secret(x) }
