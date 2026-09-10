object Main{def f[A](implicit ev:ClassManifest[A]):String=ev.toString;def main(args:Array[String]):Unit=println(f[Int])}
