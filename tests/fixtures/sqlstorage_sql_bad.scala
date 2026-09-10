import slick.jdbc.H2Profile.api._
class UnsupportedValue
object Bad {
 val q = sql"select ${new UnsupportedValue}"
}
