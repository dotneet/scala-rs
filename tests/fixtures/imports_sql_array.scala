// Renaming a selector excludes its original name from the trailing wildcard.
// This leaves scala.Array available while still importing java.sql.Array as
// SQLArray.
import java.sql.{Array => SQLArray, _}

object ImportSqlArray {
  def accepts(x: Array[Byte]): Int = x.length
  def sqlArrayType(x: SQLArray): SQLArray = x
}
