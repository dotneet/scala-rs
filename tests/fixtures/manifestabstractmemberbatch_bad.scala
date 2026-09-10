object Main{def m[A](implicit ev:scala.reflect.Manifest[A]):scala.reflect.Manifest[A]=ev;trait T{type A;def bad=m[A]}}
