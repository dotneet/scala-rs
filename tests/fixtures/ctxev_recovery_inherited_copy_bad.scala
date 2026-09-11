trait P{def copy(s:String):Int=s.length};case class C(x:Int)extends P;object Main{val c=C(7).copy(4)}
