object Main {def main(args:Array[String]):Unit={val x:Vector[String]=Vector(1,2).scanLeft(0)(_+_);println(x.head)}}
