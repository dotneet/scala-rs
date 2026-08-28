import app.model.User
import app.store.UserStore
import app.show.Show

object Main {
  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)
  def main(args: Array[String]): Unit = {
    import Show._
    val store = UserStore.of(User(1, "ann", List("a")), User(2, "bob", Nil))
    println(store.names)
    println(store.byName("ann").map(_.id))
    println(store.byName("zed").isEmpty)
    println(render(User(3, "cid", Nil)))
    println(render(store.all))
    println(User.anonymous(9))
    println(store.all.filter(_.tags.nonEmpty).map(_.name))
  }
}
