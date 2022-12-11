import http, { Agent } from "http";
import net from "net";

import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import Session from "@tsereact/node-rust-windows-native-api/io/SystemHttpSession";
import Request from "@tsereact/node-rust-windows-native-api/io/SystemHttpRequest";

NodePlugin.setup(import.meta.url);

const name = "test-v4";
const sess = Session.create(name);
sess.listen("http://localhost:9480/");

const r1 = Request.create(name);
r1.receive().then(x => console.log("--- r1", x));

const r2 = Request.create(name);
r2.receive().then(x => console.log("--- r2", x));

setTimeout(() => {
    sess;
    r1.close();
}, 300);

setInterval(() => {}, 300000);
