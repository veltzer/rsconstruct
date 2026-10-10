import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.Queue;
import java.util.Deque;
import java.util.Optional;

/**
 * Class doc mentions {@link List}.
 */
class Javadoc {
    /**
     * @param m a {@link
     *     Map#get(Object) value}
     * @return {@code Set} is not a reference
     * @see Queue
     */
    int f(int m) { return m; }

    // a line comment between the javadoc and the method

    /** @throws Deque#push oops */

    int g() { return 0; }

    /**{@link Optional}*/
    int h;
}
