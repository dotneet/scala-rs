package macrotransport.lazycodec

import scala.annotation.implicitNotFound

@implicitNotFound("Missing codec for ${A}")
trait Codec[A]
