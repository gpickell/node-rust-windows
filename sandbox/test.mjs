import { appendFileSync, writeFileSync } from "fs";
import NodePlugin from "@tsereact/node-rust-windows-native-api/NodePlugin";
import ServiceAPI from "@tsereact/node-rust-windows-native-api/ServiceAPI";

import wt from "worker_threads";

NodePlugin.setup(import.meta.url);

const service = ServiceAPI.create();

if (wt.isMainThread) {
    start();
    const worker = new wt.Worker(new URL(import.meta.url));
} else {
    service.post("worker-test");
}


function start() {
    //const fn = "d:\\test.log";
    //writeFileSync(fn, `--- init ${new Date()}\n`);

    let ptr = service.watch((x, ...args) => {
        console.log("---", x, ...args);
        //appendFileSync(fn, `--- ${x}\n`);

        if (x === "start") {
            service.startPending();
            service.running();
        }

        if (x === "control-stop") {
            service.clear();
            service.stopPending();
        }
    });

    process.on("exit", () => {
        console.log("--- exit");
        //appendFileSync(fn, `--- exit\n`);
        service.stopped();
        service.shutdown();
    });

    process.once("SIGINT", () => {
        console.log("--- SIGINT");
        //appendFileSync(fn, `--- SIGINT\n`);
        service.post("control-stop");
    });

    console.log("--- start", service.simulate("test_service", false));
}
