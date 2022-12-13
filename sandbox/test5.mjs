import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import Session from "@tsereact/node-rust-windows-native-api/io/SystemHttpSession";
import Request from "@tsereact/node-rust-windows-native-api/io/SystemHttpRequest";

const svc = NodePlugin.setup(import.meta.url);
const s = Session.create("qname-113")
s.listen("http://localhost:9180/test1/");
s.close();
s.listen("http://localhost:9180/test2/");
