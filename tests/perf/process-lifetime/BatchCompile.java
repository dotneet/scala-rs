import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.charset.StandardCharsets;

public final class BatchCompile {
    public static void main(String[] args) throws Exception {
        boolean nativeMode = args.length >= 5 && !args[3].startsWith("--");
        int flagsAt = nativeMode ? 5 : 3;
        boolean warmup = false;
        boolean batchNative = false;
        for (int i = flagsAt; i < args.length; i++) {
            if ("--warmup".equals(args[i])) warmup = true;
            else if ("--batch-native".equals(args[i])) batchNative = true;
            else throw new IllegalArgumentException("Unknown batch flag: " + args[i]);
        }
        if ((args.length < 3 || (nativeMode && args.length < 5))
                || (batchNative && !nativeMode)) {
            throw new IllegalArgumentException(
                "Usage: BatchCompile source classpath repeats [native-binary scala-library] "
                    + "[--warmup] [--batch-native]");
        }
        Path source = Path.of(args[0]);
        String classpath = args[1];
        int repeats = Integer.parseInt(args[2]);
        Path root = Files.createTempDirectory("compiler-batch-");
        long total = 0;
        Process batch = batchNative ? new ProcessBuilder(args[3], "__compile_batch")
            .redirectError(root.resolve("batch-native.log").toFile()).start() : null;
        try {
            for (int i = warmup ? -1 : 0; i < repeats; i++) {
                String label = i < 0 ? "warmup" : String.valueOf(i);
                Path input = Files.copy(source, root.resolve("input-" + label + ".scala"));
                Path output = Files.createDirectory(root.resolve("out-" + label));
                long start = System.nanoTime();
                boolean success;
                if (batchNative) {
                    String[] compileArgs = {input.toString(), "-cp", classpath,
                        "--scala-library", args[4], "-nowarn", "-d", output.toString()};
                    for (String arg : compileArgs) {
                        batch.getOutputStream().write(arg.getBytes(StandardCharsets.UTF_8));
                        batch.getOutputStream().write(0);
                    }
                    batch.getOutputStream().write(0);
                    batch.getOutputStream().flush();
                    success = batch.getInputStream().read() == 0;
                } else if (nativeMode) {
                    Process process = new ProcessBuilder(args[3], "compile", input.toString(),
                        "-cp", classpath, "--scala-library", args[4], "-nowarn", "-d", output.toString())
                        .redirectErrorStream(true)
                        .redirectOutput(root.resolve("native-" + label + ".log").toFile())
                        .start();
                    success = process.waitFor() == 0;
                } else {
                    success = new scala.tools.nsc.MainClass().process(
                        new String[] {"-nowarn", "-cp", classpath, "-d", output.toString(), input.toString()});
                }
                long elapsed = System.nanoTime() - start;
                if (!success) {
                    throw new IllegalStateException("Compilation failed at iteration " + i + " in " + root);
                }
                if (i >= 0) {
                    total += elapsed;
                    System.out.printf("%d %.3f%n", i, elapsed / 1e9);
                }
            }
        } finally {
            if (batch != null) {
                batch.getOutputStream().close();
                if (batch.waitFor() != 0) {
                    throw new IllegalStateException("Resident native compiler failed in " + root);
                }
            }
        }
        System.out.printf("total %.3f%n", total / 1e9);
        System.out.println(root);
    }
}
