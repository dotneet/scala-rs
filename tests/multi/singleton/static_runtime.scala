object Main { def main(args:Array[String]):Unit={
println(1.isInstanceOf[Singleton]);println(().isInstanceOf[Singleton]);println(null.isInstanceOf[Singleton]);println((null:Any).isInstanceOf[Singleton]);println((null:AnyRef).isInstanceOf[Singleton]);println((null:Singleton).isInstanceOf[Singleton]);println(null match {case _:Singleton=>"yes";case _=>"no"})
} }
