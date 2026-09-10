import scala.util.{Try, Success, Failure}
object Main {
  def flat[A](x:Option[Option[A]]):Option[A]=x.flatten
  def collect[A,B](x:Option[A],f:PartialFunction[A,B]):Option[B]=x.collect(f)
  def zip[A,B](x:Option[A],y:Option[B]):Option[(A,B)]=x.zip(y)
  def bind[A,B](x:Try[A],f:A=>Try[B]):Try[B]=x.flatMap(f)
  def change[A,B](x:Try[A],f:A=>Try[B],g:Throwable=>Try[B]):Try[B]=x.transform(f,g)
  def recover[A,B >: A](x:Try[A],f:PartialFunction[Throwable,B]):Try[B]=x.recover(f)
  def recoverWith[A,B >: A](x:Try[A],f:PartialFunction[Throwable,Try[B]]):Try[B]=x.recoverWith(f)
  def collectTry[A,B](x:Try[A],f:PartialFunction[A,B]):Try[B]=x.collect(f)
  def main(args:Array[String]):Unit = {
    println(Option(7).collect[String] {case i => "n"+i})
    println(Success(7).collect[String] {case i => "n"+i})
    println(Failure[Int](new RuntimeException("bad")).recover[Any] {case _:RuntimeException => "ok"})
    println(flat(Some(Some(7))))
    println(flat[Int](Some(None)))
    println(collect[Int,String](Some(7), {case i => "n"+i}))
    println(zip(Some(7),Some("a")))
    println(Option(7).zip[Any,String](Some("wide")))
    println(bind(Success(7),(i:Int)=>Success("n"+i)))
    println(change(Success(7),(i:Int)=>Success("n"+i),(e:Throwable)=>Success(e.getMessage)))
    println(change(Failure[Int](new RuntimeException("bad")),(i:Int)=>Success("n"+i),(e:Throwable)=>Success(e.getMessage)))
    val fallback:Try[Any] = Failure[Int](new RuntimeException("bad")).orElse(Success("fallback"))
    println(fallback)
    var called=0
    val keep:Try[Any] = Success(7).orElse { called=called+1; Success("wrong") }
    println(keep);println(called)
    println(recover[Int,Any](Failure[Int](new RuntimeException("bad")), {case _:RuntimeException => "ok"}))
    println(recoverWith[Int,Any](Failure[Int](new RuntimeException("bad")), {case _:RuntimeException => Success("ok")}))
    println(collectTry[Int,String](Success(7), {case i => "n"+i}))
  }
}
