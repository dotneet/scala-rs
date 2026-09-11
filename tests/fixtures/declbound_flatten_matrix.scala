object Main {def nested[A](x:Seq[Seq[A]]):Seq[A]=x.flatten
def main(args:Array[String]):Unit={println(nested(Seq(Seq(1,2),Seq(3))).mkString(","));println(Seq(Array(4,5),Array(6)).flatten.mkString(","));println(Seq(Some("a"),None).flatten.mkString)}}
