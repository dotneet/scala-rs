object Main{def m[A](implicit ev:scala.reflect.Manifest[A]):scala.reflect.Manifest[A]=ev;def bad[A <: Number]=m[A]}
