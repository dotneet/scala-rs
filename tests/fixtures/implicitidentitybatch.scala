import implicitidentity._
object Main {
 def read(holder:Holder):Int={ import holder._; implicitly[Evidence[String]].value }
 def main(args:Array[String]):Unit={
  println(implicitly[Evidence[Int]].value)
  val loaded=Evidence.intEvidence
  println(implicitly[Evidence[Int]].value==loaded.value)
  println(read(new Holder(21)));println(read(new Holder(22)));
  { import Inherited._; println(implicitly[Evidence[String]].value) };
  { import Values._; println(implicitly[Evidence[Double]].value);println(implicitly[Evidence[Long]].value) }
 }
}
