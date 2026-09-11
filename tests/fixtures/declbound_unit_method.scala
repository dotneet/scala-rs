class Box[T](x:T){def value:T=x};object Main{def main(args:Array[String]):Unit={val b=new Box[Unit](());var i=0;while(i<3){b.value;i+=1};println(i)}}
