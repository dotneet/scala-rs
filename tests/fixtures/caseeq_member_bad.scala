// A plain class does not acquire a `canEqual` member out of nowhere: only a
// `case` class and a `case object` get one. Kept in its own file because
// scalac stops after the typer, so this error and `caseeq_bad`'s refchecks
// error never appear in the same run.
//
// scalac 2.13.16: `value canEqual is not a member of NoEquals`.
class NoEquals(val z: Int)

object Main {
  def main(args: Array[String]): Unit = println(new NoEquals(1).canEqual(1))
}
