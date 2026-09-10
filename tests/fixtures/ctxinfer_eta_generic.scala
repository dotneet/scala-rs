object Main {def conv[A](a:A)(implicit f:A=>String):String=f(a);
def main(args:Array[String]):Unit={implicit val f:Int=>String=n=>"v"+n;
 val g:Int=>String=conv[Int];
println(g(4))}}

