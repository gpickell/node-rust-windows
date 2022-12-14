import http, { Agent } from "http";
import net from "net";

import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import Session from "@tsereact/node-rust-windows-native-api/io/SystemHttpSession";
import Request from "@tsereact/node-rust-windows-native-api/io/SystemHttpRequest";

NodePlugin.setup(import.meta.url);

const name = "test-v4";
const sess = Session.create(name);
sess.listen("http://localhost:9480/");

process.on("exit", () => {
    sess.close();
});

async function receive_it() {
    const req = Request.create(name);
    console.log("--- receive", await req.receive());
    console.log("--- state", req);

    console.log("--- receive data", await req.receiveData());
    await new Promise(x => setTimeout(x, 300));
    console.log("--- receive data", await req.receiveData());

    //req.disconnect = true;
    req.response.status = 200;
    req.response.reason = "OK";
    req.response.headers.add("Cache-Control", "no-cache");
    //req.response.headers.add("Content-Length", "12");
    req.response.headers.add("Transfer-Encoding", "chunked");
    req.response.headers.add("X-Test", "test1");
    req.response.trailers.add("X-Trailer", "test22");
    console.log("--- send", await req.send());
    console.log("--- send data", await req.sendData("test123 asdf", true));
}

receive_it();

async function try_it() {
    await new Promise(x => setTimeout(x, 300));

    const agent = new http.Agent({
        keepAlive: true,
        noDelay: true,
        path: null
    });

    let req = http.request("http://localhost:9480/", { 
        agent,
        method: "POST",
        headers: {
            "X-Test-Header": "test-value",
            //"Transfer-Encoding": "chunked",
        },        
    });

    req.on("error", () => {});

    req.on("response", res => {
        console.log("--- resposne", res.statusCode, res.statusMessage, res.httpVersion, res.rawHeaders);

        res.setEncoding("utf-8");
        res.on("data", x => console.log("--- data", x));
        res.on("end", () => console.log("--- end", res.rawTrailers));
        //res.on("close", () => console.log("--- close", res.rawTrailers));
    });
    
    req.write("test");
    req.flushHeaders();
    req.end();
}

async function try_it_2() {
    let socket = net.connect(9480, "localhost");
    socket.setEncoding("utf-8");
    socket.setDefaultEncoding("utf-8");

    socket.write("GET / HTTP/1.1\r\n");
    socket.write("Host: localhost\r\n");
    socket.write("\r\n");

    socket.end();

    socket.on("data", x => {
        console.log("--- socket data (", x);
        console.log("--- socket data )");
    });

    socket.on("end", x => console.log("--- socket end"));
    socket.on("close", x => console.log("--- socket close"));
    
}

try_it();
setInterval(() => {}, 3000);
