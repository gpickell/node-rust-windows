import { appendFileSync, writeFileSync } from "fs";
import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";

const svc = NodePlugin.setup(import.meta.url);

const fn = "d:\\test.log";
writeFileSync(fn, `--- init ${new Date()}\n`);

let ptr = svc.service_watch(x => {
    console.log("---", x);
    appendFileSync(fn, `--- ${x}\n`);

    if (x === "start") {
        svc.service_start_pending();
        svc.service_running();
    }

    if (x === "control-stop") {
        svc.service_clear();
        svc.service_stop_pending();
    }
});

process.on("exit", () => {
    console.log("--- exit");
    appendFileSync(fn, `--- exit\n`);
    svc.service_stopped();
    svc.service_shutdown();
});

process.once("SIGINT", () => {
    console.log("--- SIGINT");
    appendFileSync(fn, `--- SIGINT\n`);
    svc.service_post("control-stop");
});

console.log("--- start", svc.service_simulate("test_service", false));
