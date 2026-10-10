import java.io.*;

class Resources {
    void f() throws IOException {
        try (InputStream a = new FileInputStream("a"); InputStream b = new FileInputStream("b")) {
            a.read();
        }
        try (InputStream c = new FileInputStream("c");
             InputStream d = new FileInputStream("d");) {
            c.read();
        }
        InputStream e = null;
        try (e) {
            e.read();
        }
    }
}
