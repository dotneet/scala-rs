object Main { trait Has[T]; class G[T](val x:T) { self:Has[T]=> }; val g=new G(1) }
