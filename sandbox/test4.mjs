import http, { Agent } from "http";
import net from "net";

import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import Session from "@tsereact/node-rust-windows-native-api/io/SystemHttpSession";
import Request from "@tsereact/node-rust-windows-native-api/io/SystemHttpRequest";

const svc = NodePlugin.setup(import.meta.url);
const result = svc.user_groups("viaProcess", true);
console.log(svc.user_groups("viaProcess", true));

const final = await Promise.all(result.map(x => Promise.all(x)))
console.log(final);
