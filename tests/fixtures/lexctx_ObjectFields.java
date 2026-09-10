class ObjectFields<T> {
 public static final Object MARKER=new Object();
 public static Object shared=MARKER;
 public Object value=MARKER;
 public T generic;
 public Object[] array=new Object[]{"item"};
 public static Object echo(Object value){return value;}
}
