object Main {def main(args:Array[String]):Unit={val x=for {a<-List(1,2,3);b=a+1;if b%2==0;c=b*2;if c<8}yield(a,b,c);println(x.mkString(","))}}
