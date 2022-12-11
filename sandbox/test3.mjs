import http from "http";

import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import Manager from "@tsereact/node-rust-windows-native-api/io/SystemHttpManager";
import Session from "@tsereact/node-rust-windows-native-api/io/SystemHttpSession";

NodePlugin.setup(import.meta.url);

setInterval(() => {}, 30000);

const name = "test-v4";
const manager = new Manager();
manager.createSession(name);
manager.process(name);

process.on("exit", () => {
    manager.close();
});

console.log("--- listen", manager.listen("http://localhost:9480/"));

const pin = new Set();
pin.add(manager);

const server = http.createServer();
manager.on("handoff", x => server.emit("connection", x));

server.on("request", (req, res) => {
    console.log("--- server request", req.method, req.url, req.httpVersion, req.headers);
    
    req.setEncoding("utf-8");
    req.resume();
    req.on("data", x => console.log("--- server data", JSON.stringify(x)));
    req.on("end", x => {
        console.log("--- server end");

        res.statusCode = 200;
        res.write("Some Server Content");
        res.end();
    });

    //res.statusCode = 200;
    //res.write("Some Server Content");
    //res.end();
});

async function test(ms) {
    await new Promise(x => setTimeout(x, ms));

    const req = http.request("http://localhost:9480/", { method: "POST" });
    req.on("error", x => console.log("--- client error", x.message));

    req.on("response", res => {
        res.setEncoding("utf-8");
        res.resume();

        console.log("--- client", res.httpVersion, res.statusCode, res.statusMessage, res.headers);
        
        res.on("data", x => console.log("--- client data", JSON.stringify(x)));
        res.on("end", x => console.log("--- client end"));
    });

    req.write("Some Client Content");
    req.end();
}

test(300);
test(1300);
