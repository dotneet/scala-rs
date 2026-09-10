object Main { class Base {def name:String="base"};def bad(xs:List[_ >: Base]):String=xs.head.name }
