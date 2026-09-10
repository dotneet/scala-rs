trait TC[A]{def value:Int};
trait Strong[A] extends TC[A];
trait W[A]{implicit def algebra:TC[A];
def get:Int=algebra.value};
object Main{def make[A:Strong]:W[A]=new W[A]{val algebra:Strong[A]=implicitly[Strong[A]]};
def main(args:Array[String]):Unit={implicit val t:Strong[Int]=new Strong[Int]{def value:Int=9};
println(make[Int].get)}}

