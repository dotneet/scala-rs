trait H {val f:()=>String};object Main {val build=(s:Int)=>new H{val f=()=>s}}
