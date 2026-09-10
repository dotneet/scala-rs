object Main{implicit val m:scala.reflect.Manifest[Nothing]=scala.reflect.Manifest.Nothing;val bad=implicitly[scala.reflect.Manifest[Int]](m)}
