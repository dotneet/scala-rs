object Main { class Base;class Child extends Base {def childOnly:Int=1};def bad(xs:List[_ <: Base]):Int=xs.head.childOnly }
