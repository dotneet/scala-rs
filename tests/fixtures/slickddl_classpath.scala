// Minimal consumer of the classfile API exported by Slick's compiled sources.
// The same source succeeds when Slick is compiled in the invocation, but
// exercises the classpath path used by slick-testkit when Slick is supplied as
// previously emitted classfiles.
import slickddlmini.Profile

class SlickDdlClasspathUse(val profile: Profile) {
  import profile.api._
  val schema: profile.SchemaDescription = ???
  val combined = schema ++ schema
  val createAction = schema.create
}
