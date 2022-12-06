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
    const header = await req.receive();
    console.log("--- js receive", header);

    const data1 = await req.receiveData(header.id);
    console.log("--- js receive data", data1);

    const data2 = await req.receiveData(header.id);
    console.log("--- js receive data", data2);
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
        res.resume();
    });
    
    req.write("test");
    req.flushHeaders();
    req.end();
}

try_it();
setInterval(() => {}, 3000);
