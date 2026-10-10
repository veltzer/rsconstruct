package com.example.app;

import java.util.List;
import java.util.Map;
import java.util.Set;
import java.lang.String;
import java.lang.annotation.Retention;
import com.example.app.Helper;
import java.util.List;
import java.io.*;
import static java.lang.Math.max;
import static java.lang.Math.max;
import static java.util.Collections.emptyList;
import java.util.function.Function;
import java.util.concurrent.Callable;
import java.time.Duration;
import java.nio.file.Path;

/**
 * Uses {@link Function} and {@linkplain Callable the callable}.
 * @see Duration#ofSeconds(long, Path)
 * @throws java.io.IOException never
 */
public class Imports {
    private Map<String, Integer> counts;

    /** {@value Set#EMPTY} */
    int size() {
        return max(1, 2) + emptyList().size();
    }
}
