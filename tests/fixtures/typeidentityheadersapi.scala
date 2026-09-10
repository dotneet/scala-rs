package identityheaders {
 import identitybase._
 trait Bounded[A <: Base]
}
package identitybase {
 trait Base
 class Child extends Base
}
