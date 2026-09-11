// An implementation's verdict on a class this run is compiling.
//
// `MgNameImpl.of` asks its type argument what it *is*, the way slick's
// `mapToImpl` does, and aborts unless it is a case class. `MgPlain` is not.
// Real scalac 2.13.16 reports `MgPlain must be a case class` and
// `java.lang.String must be a case class`, and so must scala-rs.
//
// This file used to pin the opposite. A class this run is compiling went to
// the engine as a placeholder carrying its name and no info, so the verdict
// was about a symbol the implementation had never been shown, and it was
// replaced by a note saying so. The class now goes over as its identity and
// is described in nsc's shape when asked (`docs/macros.md` §7.25): the
// implementation sees the real class, and its judgement is the program's
// error, exactly as under nsc.
//
// The second call is the control: `java.lang.String` is on the macro
// classpath, and its verdict has always been reported as itself.
import mgl.MgName

class MgPlain(x: Int)

object Main {
  val fromLocal = MgName.of[MgPlain]
  val fromJar = MgName.of[String]

  def main(args: Array[String]): Unit = {
    println(fromLocal)
    println(fromJar)
  }
}
