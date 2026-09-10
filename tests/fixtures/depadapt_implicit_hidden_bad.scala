trait TC[A];trait Wrap[A] {implicit def algebra:TC[A]};object Main {val x=new Wrap[Int] {val algebra:TC[Int]=new TC[Int] {};val bad:TC[Int]=implicitly[TC[Int]]} }
