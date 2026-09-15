import scala.async.Async.{async, await}
import scala.concurrent.{ExecutionContext, Future}
import scala.concurrent.ExecutionContext.Implicits.global
object BadAsync {
  def delayed(x: => Int): Int = x
  val outside = await(Future.successful(1))
  val lambda = async { List(1).map(x => await(Future.successful(x))) }
  val method = async { def f(): Int = await(Future.successful(1)); f() }
  val nestedClass = async { class C { val x = await(Future.successful(1)) }; new C }
  val nestedObject = async { object C { val x = await(Future.successful(1)) }; C.x }
  val tried = async { try { await(Future.successful(1)) } catch { case _: Exception => 0 } }
  val lazyValue = async { lazy val x = await(Future.successful(1)); x }
  val byName = async { delayed(await(Future.successful(1))) }
}
