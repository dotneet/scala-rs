package asynchook
import scala.language.experimental.macros
import scala.reflect.macros.blackbox
object Generic {
  def optionally[T](body: T): Option[T] = macro impl
  def value(x: Int): Int = x + 1
  @scala.annotation.compileTimeOnly("[async] value must be enclosed in optionally")
  def value[T](x: Option[T]): T = ???
  def impl(c: blackbox.Context)(body: c.Tree): c.Tree = {
    import c.universe._
    val awaitSym = typeOf[Generic.type].decl(TermName("value")).alternatives.find(_.asMethod.typeParams.nonEmpty).get
    val marked = c.internal.markForAsyncTransform(c.internal.enclosingOwner,
      q"override def apply(tr: _root_.scala.Option[_root_.scala.AnyRef]) = $body", awaitSym, Map.empty)
    q"""{ final class Machine extends _root_.asynchook.OptionalMachine { ${marked.duplicate} }; new Machine().start().asInstanceOf[${c.macroApplication.tpe}] }"""
  }
}
abstract class OptionalMachine {
  private var s = 0
  private var result: Option[AnyRef] = None
  protected def state: Int = s
  protected def state_=(n: Int): Unit = s = n
  def apply(tr: Option[AnyRef]): Unit
  protected def completeSuccess(x: AnyRef): Unit = result = Some(x)
  protected def completeFailure(t: Throwable): Unit = throw t
  protected def onComplete(f: Option[AnyRef]): Unit = throw new AssertionError("unexpected suspension")
  protected def getCompleted(f: Option[AnyRef]): Option[AnyRef] = f
  protected def tryGet(tr: Option[AnyRef]): AnyRef = tr match {
    case Some(x) => x
    case None => result = None; this
  }
  def start(): Option[AnyRef] = { apply(None); result }
}

object ForeignFuture {
  def brokenComplete[T](body: T): java.util.concurrent.CompletableFuture[T] = macro brokenCompleteImpl
  def brokenCompleteImpl(c: blackbox.Context)(body: c.Tree): c.Tree = {
    import c.universe._
    val awaitSym = typeOf[ForeignFuture.type].decl(TermName("await"))
    val marked = c.internal.markForAsyncTransform(c.internal.enclosingOwner,
      q"override def apply(tr: _root_.scala.util.Try[_root_.scala.AnyRef]) = $body", awaitSym, Map.empty)
    q"""{
      final class Machine extends _root_.asynchook.ForeignMachine {
        override protected def completeSuccess(value: _root_.scala.AnyRef): _root_.scala.Unit = throw new _root_.java.lang.IllegalStateException("success-hook")
        $marked
      }
      new Machine().start().asInstanceOf[${c.macroApplication.tpe}]
    }"""
  }
  def polling[T](body: T): java.util.concurrent.CompletableFuture[T] = macro pollingImpl
  def pollingImpl(c: blackbox.Context)(body: c.Tree): c.Tree = {
    import c.universe._
    val awaitSym = typeOf[ForeignFuture.type].decl(TermName("await"))
    val marked = c.internal.markForAsyncTransform(c.internal.enclosingOwner,
      q"override def apply(completion: _root_.scala.util.Try[_root_.scala.AnyRef]) = $body", awaitSym, Map.empty)
    q"""{
      final class Machine extends _root_.asynchook.ForeignMachine {
        def getCompleted(f: _root_.java.util.concurrent.CompletableFuture[_root_.scala.AnyRef]): _root_.scala.util.Try[_root_.scala.AnyRef] = {
          _root_.scala.Predef.println("polled")
          if (f.isDone()) _root_.scala.util.Success(f.get()) else null
        }
        $marked
      }
      new Machine().start().asInstanceOf[${c.macroApplication.tpe}]
    }"""
  }
  def propagating[T](body: T): java.util.concurrent.CompletableFuture[T] = macro propagateImpl
  def propagateImpl(c: blackbox.Context)(body: c.Tree): c.Tree = {
    import c.universe._
    val awaitSym = typeOf[ForeignFuture.type].decl(TermName("await"))
    val marked = c.internal.markForAsyncTransform(c.internal.enclosingOwner,
      q"override def apply(tr: _root_.scala.util.Try[_root_.scala.AnyRef]) = $body", awaitSym,
      Map("allowExceptionsToPropagate" -> java.lang.Boolean.TRUE, "ignoredExtension" -> "ignored"))
    q"""{ final class Machine extends _root_.asynchook.ForeignMachine { $marked }; new Machine().start().asInstanceOf[${c.macroApplication.tpe}] }"""
  }
  def async[T](body: T): java.util.concurrent.CompletableFuture[T] = macro impl
  @scala.annotation.compileTimeOnly("[async] await must be enclosed in async")
  def await[T](f: java.util.concurrent.CompletableFuture[T]): T = ???
  def impl(c: blackbox.Context)(body: c.Tree): c.Tree = {
    import c.universe._
    val awaitSym = typeOf[ForeignFuture.type].decl(TermName("await"))
    val marked = c.internal.markForAsyncTransform(c.internal.enclosingOwner,
      q"override def apply(incoming: _root_.scala.util.Try[_root_.scala.AnyRef]) = { val starting = incoming == null; $body }", awaitSym, Map.empty)
    q"""{ final class Machine extends _root_.asynchook.ForeignMachine { $marked }; new Machine().start().asInstanceOf[${c.macroApplication.tpe}] }"""
  }
}

// Deliberately has no getCompleted: the protocol allows callback-only libraries.
abstract class ForeignMachine extends java.util.function.BiConsumer[AnyRef, Throwable] {
  private var s = 0
  private val result = new java.util.concurrent.CompletableFuture[AnyRef]()
  protected def state: Int = s
  protected def state_=(n: Int): Unit = s = n
  def apply(tr: scala.util.Try[AnyRef]): Unit
  def accept(value: AnyRef, error: Throwable): Unit = {
    if (error == null) apply(scala.util.Success(value))
    else apply(scala.util.Failure(error))
  }
  protected def completeSuccess(x: AnyRef): Unit = { result.complete(x); () }
  protected def completeFailure(t: Throwable): Unit = { result.completeExceptionally(t); () }
  protected def onComplete(f: java.util.concurrent.CompletableFuture[AnyRef]): Unit = { f.whenComplete(this); () }
  protected def tryGet(tr: scala.util.Try[AnyRef]): AnyRef = tr match {
    case scala.util.Success(x) => x
    case scala.util.Failure(t) => completeFailure(t); this
  }
  def start(): java.util.concurrent.CompletableFuture[AnyRef] = { apply(null); result }
}
