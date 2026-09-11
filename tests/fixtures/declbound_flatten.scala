object Main {def main(args:Array[String]):Unit={println(Seq(Seq(1,2),Seq(3)).flatten.map(_+1).mkString(","));println(Seq(Some("a"),None).flatten.mkString)}}
