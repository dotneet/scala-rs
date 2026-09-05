// The honest limit of the placeholder (`docs/macros.md` §5.1).
//
// A class this run is compiling goes to a macro implementation as a symbol
// carrying its full name and no info: scala-rs cannot describe it truthfully
// at that point in its own run -- while `lazy val rows = MgQuery[Row]` above
// is being typed, the members of `class Row` are still un-inferred. So an
// implementation that asks the placeholder *what the class is* is answering
// about a symbol it was never shown.
//
// `MgNameImpl.of` asks exactly that, the way slick's `mapToImpl` does. Its
// verdict on `MgPlain` -- "must be a case class" -- is about the placeholder,
// not about this program, so it must not be reported as the program's error.
// The call site is still an error: it is a macro that could not be expanded,
// with the reason.
//
// The second call is the control. `java.lang.String` really is on the macro
// classpath, so the implementation gets the true symbol and its `abort` is the
// implementation's own judgement of a real class. That one *is* reported as
// itself.
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
