import http from "http";

import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import Session from "@tsereact/node-rust-windows-native-api/io/SystemHttpSession";

NodePlugin.setup(import.meta.url);

const sess = Session.create("test-v4");
sess.listen("http://localhost:9480/");

const queue = Session.open("test-v4");

process.on("exit", () => {
    sess.close();
    queue.close();
});

async function receive_it() {
    const req = queue.request();
    console.log("--- receive", await req.receive());
    console.log("--- state", req);

    console.log("--- receive data", await req.receiveData());
    console.log("--- state", req);

    console.log("--- receive data", await req.receiveData());
    console.log("--- state", req);

    req.response.status = 200;
    req.response.reason = "OK";
    req.response.addHeader("Cache-Control", "no-cache");
    req.response.addHeader("X-Test", "test1");
    console.log("--- send", await req.send(true));
}

receive_it();

async function try_it() {
    await new Promise(x => setTimeout(x, 300));

    let req = http.request("http://localhost:9480/", { 
        method: "POST",
        headers: {
            "X-Test-Header": "test-value"
        }
    });

    req.on("error", () => {});

    req.on("response", res => {
        console.log("--- resposne", res.statusCode, res.statusMessage, res.httpVersion, res.rawHeaders);

        res.setEncoding("utf-8");
        res.on("data", x => console.log("--- data", x));
        res.on("end", x => console.log("--- end"));
    });
    
    req.write("test");
    req.flushHeaders();
    req.end();
}

try_it();
setInterval(() => {}, 3000);
