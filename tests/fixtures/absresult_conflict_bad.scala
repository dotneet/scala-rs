trait Factory[+C] { def empty: C }
class Base extends Factory[Base] { def empty: Base = new Base }
class Child extends Base with Factory[Child]
