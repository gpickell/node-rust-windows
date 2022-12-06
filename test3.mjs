import http from "http";
import net from "net";

const server = http.createServer((req, res) => {
    req.resume();
    res.statusCode = 200;
    res.addTrailers({
        // "X-Test-1": "123",
        // "X-Test-2": "123",
    });
    res.flushHeaders();
    res.write("test");
    res.end();
});

server.listen(9480, "localhost");

async function try_it() {
    await new Promise(x => setTimeout(x, 300));

    const socket = net.connect(9480, "localhost");
    socket.write("GET / HTTP/1.1\r\n\r\n");
    socket.end();
    
    socket.on("data", data => process.stdout.write(data));
    socket.on("end", data => process.stdout.write("----"));
}

try_it();
setInterval(() => {}, 3000);