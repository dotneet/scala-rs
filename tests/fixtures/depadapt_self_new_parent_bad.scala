object Main { trait A; trait B; class C extends B { self:A=> }; val a=new C }
