class Box[T](val value:T) {lazy val late:T=value}
object Main {def main(args:Array[String]):Unit={val b=new Box[Unit](());var i=0;while(i<3){b.value;b.late;if(i==1)b.value else b.late;i+=1};println(i)}}
