trait TC[A]{def value:Int};
 trait W[A]{implicit def algebra:TC[A];
def get:Int=algebra.value};
 object Main{def make[A:TC]:W[A]=new W[A]{val algebra=implicitly[TC[A]]};
def main(args:Array[String]):Unit={implicit val t:TC[Int]=new TC[Int]{def value:Int=7};
println(make[Int].get)}}

