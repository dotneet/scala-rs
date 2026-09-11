object Main {def main(args:Array[String]):Unit={val x=Seq(Seq(1,2),Seq(3));println(x.flatMap(identity).map(_+1).mkString(","));println(Seq(Some("a"),None).flatMap(identity).mkString)}}
