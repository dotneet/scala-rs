class From
class To[T] { def foo(t:T):T=t }
object Main {
 implicit def conv[T](x:From):To[T]=new To[T]
 def main(args:Array[String]):Unit={val from=new From;println(from.foo(23));println(from.foo("hi"))}
}
