object Main {
 def count:Int={List(1,2,3).foreach(n=>if(n==2)return n);0}
 def main(args:Array[String]):Unit=println(count)
}
