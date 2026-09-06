object Main {
 def f(x:Any):String=x match {case _:Singleton=>"yes";case _=>"no"}
 def instance(x: Any): Boolean = x.isInstanceOf[Singleton]
 def narrow[T <: Singleton](x:T):T=x
 def id(x:Any):Singleton=x
 def main(args:Array[String]):Unit={
 println(f(1));println(f(new Object));println(f(null));println(id("stable"));println(narrow(1));println(instance(1));println(instance(new Object));println(instance(null));println({println("effect"); null}.isInstanceOf[Singleton])
 }
}
