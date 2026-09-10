object Main {def main(args:Array[String]):Unit={def f[A](a:List[A])=a.to(scala.collection.immutable.ArraySeq);println(f(List(1)))}}
