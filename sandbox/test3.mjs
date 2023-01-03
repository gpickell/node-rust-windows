import http, { Agent, IncomingMessage, ServerResponse } from "http";

import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import Manager from "@tsereact/node-rust-windows-native-api/io/SystemHttpManager";
import PushAPI from "@tsereact/node-rust-windows-native-api/PushAPI";

NodePlugin.setup(import.meta.url);

setInterval(() => {}, 30000);

PushAPI.patch(ServerResponse.prototype, x => PushAPI.find(x.socket));

const name = "test-v4";
const manager = new Manager();
manager.createSession(name);
manager.process(name);

manager.on("relay-request", x => x.exposePush());

process.on("exit", () => {
    manager.close();
});

console.log("--- listen", manager.listen("http://*:9480/"));

const pin = new Set();
pin.add(manager);

const server = http.createServer();
server.listen(9580, "localhost");

manager.on("socket-handoff", x => server.emit("connection", x.socket));

server.on("request", (req, res) => {
    console.log("--- server request", req.method, req.url, req.httpVersion, req.headers);
    
    req.setEncoding("utf-8");
    req.resume();
    req.on("data", x => console.log("--- server data", JSON.stringify(x)));
    req.on("end", x => {
        console.log("--- server end");

        const push = PushAPI.find(res);
        push.get("/static.html", "X-Test", "test-push-1");

        res.statusCode = 200;
        res.setHeader("X-WWW-Authenticate", ["Negotiate", "NTLM"])
        res.write("Some Server Content");
        res.end();
    });

    //res.statusCode = 200;
    //res.write("Some Server Content");
    //res.end();
});

const agent = new Agent({ keepAlive: true });

async function test(ms) {
    await new Promise(x => setTimeout(x, ms));

    const req = http.request("http://localhost:9480/", { agent, method: "POST" });
    req.on("error", x => console.log("--- client error", x.message));

    req.on("response", res => {
        res.setEncoding("utf-8");
        res.resume();

        console.log("--- client", res.httpVersion, res.statusCode, res.statusMessage, res.rawHeaders);
        
        res.on("data", x => console.log("--- client data", JSON.stringify(x)));
        res.on("end", x => console.log("--- client end"));
    });

    req.setHeader("X-Test-Client", "custom-header-1");
    req.write("Some Client Content");
    req.end();
}

test(300);
test(1300);

//test2(1300);
