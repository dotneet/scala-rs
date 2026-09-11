object Main{def f():Unit={trait P{def copy(s:String):Int=s.length};case class C(x:Int)extends P;println(C(7).copy(4))}}
