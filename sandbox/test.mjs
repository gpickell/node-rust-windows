import { createRequire } from "module";
import { fileURLToPath } from "url";
import { appendFileSync, writeFileSync } from "fs";

const require = createRequire(fileURLToPath(import.meta.url));
const svc = require("./test.node");

const fn = "d:\\test.log";
writeFileSync(fn, `--- init ${new Date()}\n`);

let ptr = svc.watch(x => {
    console.log("---", x);
    appendFileSync(fn, `--- ${x}\n`);

    if (x === "start") {
        svc.startPending();
        svc.running();
    }

    if (x === "control-stop") {
        svc.clear();
        svc.stopPending();
    }
});

process.on("exit", () => {
    console.log("--- exit");
    appendFileSync(fn, `--- exit\n`);
    svc.stopped();
    svc.shutdown();
});

process.once("SIGINT", () => {
    console.log("--- SIGINT");
    appendFileSync(fn, `--- SIGINT\n`);
    svc.post("control-stop");
});

svc.simulate("test_service", false);
