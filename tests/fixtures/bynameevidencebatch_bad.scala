object Main{def take(body: => Unit):Unit=body;def f[A](xs:List[A]):Unit=take {val row=xs.toMap;println(row)}}
