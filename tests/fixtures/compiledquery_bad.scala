import compiledquery._

object BadCompiledQuery {
  // No Shape[Long, Long, Long, _] is available, so the same classfile API
  // must reject this call rather than materializing an unapplied function.
  val missing = Parameters[Long]
}

object BadRecursiveImplicit {
  val loops = implicitly[recursive.Loop[Int]]
}
