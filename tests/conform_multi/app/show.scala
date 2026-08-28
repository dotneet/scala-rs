package app.show

import app.model.User

trait Show[A] { def show(a: A): String }

object Show {
  implicit val userShow: Show[User] = new Show[User] {
    def show(u: User): String = u.id.toString + ":" + u.name
  }
  implicit def listShow[A](implicit s: Show[A]): Show[List[A]] =
    new Show[List[A]] { def show(as: List[A]): String = as.map(s.show).mkString(",") }
}
