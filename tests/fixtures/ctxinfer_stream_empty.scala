object Main{def empty[A]:Stream[A]=Stream.Empty;
def main(args:Array[String]):Unit={println(empty[Int].size);
println(empty[String].isEmpty)}}
