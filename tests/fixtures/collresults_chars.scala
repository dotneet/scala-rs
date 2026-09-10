object Main {def main(args:Array[String]):Unit={val a=new Array[Char](2);"abcd".getChars(1,3,a,0);println(a.mkString);println("ab".codePointAt(1))}}
