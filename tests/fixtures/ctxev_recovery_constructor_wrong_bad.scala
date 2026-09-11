abstract class Base[A];class Narrow[A]extends Base[A];class C[A](val x:Narrow[A]){def this(x:Base[A],y:Int)=this(x.asInstanceOf[Narrow[A]])};object Main{val bad:C[String]=new C[Int](new Narrow[Int])}
