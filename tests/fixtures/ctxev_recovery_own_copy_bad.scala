case class C(x:Int){def copy(s:String):C=C(s.length)};object Main{val c=C(7).copy(4)}
