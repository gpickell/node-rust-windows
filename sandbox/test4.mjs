import http, { Agent } from "http";
import net from "net";

import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import Session from "@tsereact/node-rust-windows-native-api/io/SystemHttpSession";
import Request from "@tsereact/node-rust-windows-native-api/io/SystemHttpRequest";

const svc = NodePlugin.setup(import.meta.url);
console.log(svc.user_claims("viaProcess", true));
