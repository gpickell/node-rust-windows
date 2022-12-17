import http, { Agent } from "http";

import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import Manager from "@tsereact/node-rust-windows-native-api/io/SystemHttpManager";
import UserAPI from "@tsereact/node-rust-windows-native-api//UserAPI";

NodePlugin.setup(import.meta.url);

UserAPI.patch(http.IncomingMessage.prototype, x => UserAPI.find(x.socket));

setInterval(() => {}, 30000);

const name = "test-v4";
const manager = new Manager();
manager.createSession(name);
manager.config("auth-extended", "negotiate", "ntlm");
manager.process(name);

process.on("exit", () => {
    manager.close();
});

console.log("--- listen", manager.listen("http://localhost:9480/"));
console.log("=== listen http://localhost:9480/");

manager.on("relay-request", info => {
    console.log("--- relay-request", !!info.exposeIdentity(true).length);
});

const server = http.createServer();
server.listen(9580, "localhost");

manager.on("socket-handoff", x => server.emit("connection", x.socket));

server.on("request", (req, res) => {
    console.log("--- server request", req.method, req.url, req.httpVersion, req.headers);

    req.setEncoding("utf-8");
    req.resume();
    req.on("data", x => console.log("--- server data", JSON.stringify(x)));
    req.on("end", async x => {
        const user = await UserAPI.resolve(req);
        console.log("--- user", user);

        console.log("--- server end");

        res.statusCode = user.length ? 200 : 401;
        res.setHeader("Content-Type", "text/plain");
        res.setHeader("WWW-Authenticate", ["Negotiate", "NTLM"]);
        res.write("Some Server Content");
        res.end();
    });

    //res.statusCode = 200;
    //res.write("Some Server Content");
    //res.end();
});

