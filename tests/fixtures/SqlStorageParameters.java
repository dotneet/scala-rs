import java.lang.reflect.Proxy;
import java.sql.PreparedStatement;
public final class SqlStorageParameters {
    public static PreparedStatement create() {
        return (PreparedStatement) Proxy.newProxyInstance(
            SqlStorageParameters.class.getClassLoader(),
            new Class<?>[]{PreparedStatement.class},
            (proxy, method, args) -> {
                if (method.getName().equals("setInt") || method.getName().equals("setString")) {
                    System.out.println(method.getName() + ":" + args[0] + ":" + args[1]);
                    return null;
                }
                throw new AssertionError("Unexpected JDBC operation: " + method.getName());
            });
    }
}
