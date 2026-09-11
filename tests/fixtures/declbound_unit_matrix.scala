class Box[T](val value:T) {lazy val late:T=value}
class Direct {val value:Unit=();lazy val late:Unit=()}
object Main {def main(args:Array[String]):Unit={val b=new Box[Unit](());val d=new Direct;var i=0
while(i<3){b.value;b.late;d.value;d.late;if(i==1)b.value else b.late;i+=1}
println(i);println(b.value);println(b.late);println(d.value);println(d.late)}}
