object Main { def bad:Unit=try {throw new RuntimeException} catch {case e => val x:String=e;println(x)} }
