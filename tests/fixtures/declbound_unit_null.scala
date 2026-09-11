class Box[T](val value:T){lazy val late:T=value}
object Main{def main(args:Array[String]):Unit={val b=new Box[Any](null).asInstanceOf[Box[Unit]];println(b.value);println(b.late)}}
