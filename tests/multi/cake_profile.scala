package cake.jdbc

import cake.relational.{Ref, RelationalProfile, TableSupport}

/** The leaf of a four-deep cake. `Table` and `Sequence` are declared in
  * components that reach `RelationalProfile` through a self-type, in files
  * that come *later* on the command line — the whole point of the test. */
trait JdbcProfile extends RelationalProfile

object DB2Profile extends JdbcProfile with TableSupport.MultipleRows {
  def profileName: String = "db2"

  /** An inner class whose parent is itself an inherited inner class, with a
    * constructor whose signature lives in a later file. */
  class PersonTable(n: String) extends Table[Int](n)

  def createTableDDL(t: Table[?]): String = "create table " + t.tableName
  def createSeqDDL(s: Sequence[?]): String = "create sequence " + s.seqName
}

/** `Ref` is a companion pair: the prefix of `Ref.Typed` is the object, even
  * though the trait of the same name is found first. */
class IntRef(l: String) extends Ref.Typed[Int](l)

object Main {
  def main(args: Array[String]): Unit = {
    println(DB2Profile.createTableDDL(new DB2Profile.PersonTable("people")))
    println(DB2Profile.createSeqDDL(new DB2Profile.Sequence[Int]("ids")))
    println(DB2Profile.rowsPerStatement)
    println(DB2Profile.tableProvider + ":" + new IntRef("int").label)
  }
}
