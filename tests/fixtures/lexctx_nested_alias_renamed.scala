abstract class Eval[A] { self =>
 def value: A
 def flatMap[B](f: A => Eval[B]): Eval[B] = this match {
  case c: Eval.FlatMap[A] => new Eval.FlatMap[B] {
   type Start = c.Start
   val start: () => Eval[Start] = c.start
   val run: Start => Eval[B] = (s:c.Start) => new Eval.FlatMap[B] {
    type Start = A
    val start = () => c.run(s)
    val run: Start => Eval[B] = f
   }
  }
  case c: Eval.Defer[A] => new Eval.FlatMap[B] {
   type Start = A
   val start = c.thunk
   val run: Start => Eval[B] = f
  }
  case _ => new Eval.FlatMap[B] {
   type Start = A
   val start = () => self
   val run: Start => Eval[B] = f
  }
 }
}
object Eval {
 class Now[A](val value:A) extends Eval[A]
 class Defer[A](val thunk:()=>Eval[A]) extends Eval[A] {def value:A=thunk().value}
 abstract class FlatMap[T] extends Eval[T] {
  type Start
  val start:()=>Eval[Start]
  val run:Start=>Eval[T]
  def value:T=run(start().value).value
 }
}
object Main {def main(args:Array[String]):Unit={
 val x:Eval[Int]=new Eval.Defer[Int](()=>new Eval.Now(2))
 val y=x.flatMap(n=>new Eval.Now(n+1))
 println(y.value)
 println(y.flatMap(n=>new Eval.Now(n+1)).value)
}}
