// The library half of the classpath descriptor regression.
package descriptorholder

trait Base {
  val api: descriptor.Api = null
}

object Holder extends Base
