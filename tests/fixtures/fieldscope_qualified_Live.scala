package bbb
object Live { private[bbb] def secret(x:Int):Int=x+1;val f:Int=>Int=(x:Int)=>Live.secret(x) }
