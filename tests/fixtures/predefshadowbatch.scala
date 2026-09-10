object Main{object Imports{type Manifest[A]=List[A];def manifest[A]:List[A]=Nil};import Imports._;def main(args:Array[String]):Unit={val xs:Manifest[Int]=manifest[Int];println(xs.size)}}
