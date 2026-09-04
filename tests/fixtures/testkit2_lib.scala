package testkit2lib

// The shape a slick profile exports its API in, reduced to what a classfile
// reader has to carry across a separate compilation:
//
//   * `Table` is a *nested* class with a primary and a secondary constructor.
//     nsc writes no `ScalaSignature` on a nested class's own class file (only
//     a zero-length `Scala` marker), so a reader that looks only there finds
//     no constructor at all.
//   * `O` has the singleton type of another `val` (`Profile.opts.type`),
//     which is how `RelationalTableComponent#Table` declares its own `O`.
//   * `api` exports a *parameterised* alias (`type Table[T]`) and a *nullary*
//     one (`type Tag`); a nullary alias has no symbol of its own -- it is its
//     right-hand side.
//   * `describe` is inherited by whatever extends `Table`, and the subclass
//     lives in another compilation unit.
class Tag(val path: String)

object Profile {
  trait Opts { val PrimaryKey: String = "pk" }
  val opts: Opts = new Opts {}

  abstract class Table[T](val tag: Tag, val schemaName: Option[String], val tableName: String) {
    def this(tag: Tag, tableName: String) = this(tag, None, tableName)

    val O: Profile.opts.type = opts

    def describe: String =
      schemaName.getOrElse("_") + "." + tableName + "@" + tag.path + "/" + O.PrimaryKey
  }

  object api {
    type Table[T] = Profile.Table[T]
    type Tag = testkit2lib.Tag
    val O: Profile.opts.type = Profile.opts
  }
}
