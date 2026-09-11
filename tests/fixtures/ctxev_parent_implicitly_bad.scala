trait Evidence[A];class Parent[A](val ev:Evidence[A]);class Child[A](implicit ev:Evidence[String]) extends Parent[A](implicitly)

