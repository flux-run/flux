// hello — Flux function
// Build to WASM: ./gradlew nativeCompile  (see build.gradle — requires GraalVM JDK)
// Type-check:    javac Handler.java        (works with standard JDK 21+)
import java.nio.charset.StandardCharsets;

public class Handler {

    /**
     * Flux runtime entry point — exported as "hello_handler" in the WASM binary.
     *
     * When using GraalVM Native Image, annotate with @CEntryPoint and accept an
     * IsolateThread as the first argument. For development and type-checking, this
     * plain static method compiles with any standard JDK.
     *
     * @param inputPtr  pointer to JSON-encoded input in WASM linear memory
     * @param inputLen  byte length of the input
     * @return          (outputPtr &lt;&lt; 32) | outputLen
     */
    public static long handle(long inputPtr, int inputLen) {
        // TODO: decode JSON input from linear memory at inputPtr
        byte[] resp = "{\"ok\":true}".getBytes(StandardCharsets.UTF_8);
        // Return (outputPtr << 32) | outputLen packed into a long.
        return ((long) inputPtr << 32) | resp.length;
    }

    public static void main(String[] args) {
        // Local smoke-test
        System.out.println(handle(0, 0));
    }
}
