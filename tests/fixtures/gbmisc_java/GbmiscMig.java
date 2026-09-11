import java.util.Map;

// gitbucket's `Migration`: a Java interface whose parameter mentions
// `Object` inside a type argument. nsc reads that `Object` as
// `ObjectTpeJava`, the same type as both `Any` and `AnyRef`.
public interface GbmiscMig {
    void migrate(String moduleId, String version, Map<String, Object> context) throws Exception;
}
