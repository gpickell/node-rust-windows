import { createRequire } from "module";
import { fileURLToPath } from "url";
import { copyFileSync } from "fs";
import http from "http";

function setup() {
    const require = createRequire(fileURLToPath(import.meta.url));
    return require("./test.node");
}

let svc;

class Session {
    constructor(ref, name) {
        this.ref = ref;
        this.name = name;
    }

    static create(name) {
        if (svc === undefined) {
            svc = setup();
        }

        let ref = svc.http_session_create(name);
        return new this(ref, name);
    }

    static open(name) {
        if (svc === undefined) {
            svc = setup();
        }

        let ref = svc.http_session_open(name);
        return new this(ref, name);
    }

    isController() {
        return svc.http_session_is_controller(this.ref);
    }

    close() {
        svc.http_session_close(this.ref);
        this.ref = undefined;
    }

    listen(url) {
        svc.http_session_listen(this.ref, url);
    }

    async request() {
        let ref = svc.http_session_request(this.ref);
        return new Request(ref);
    }
}

class Request {
    constructor(ref) {
        this.ref = ref;
    }

    async receive(id, size) {
        return svc.http_request_receive(this.ref, id, size);
    }
}

copyFileSync("target/release/hello_world.dll", "test.node");
await new Promise(x => setTimeout(x, 100));

const sess = Session.create("test-v4");
sess.listen("http://localhost:9480/");

const queue = Session.open("test-v4");

process.on("exit", () => {
    sess.close();
    queue.close();
});

async function receive_it() {
    const req = await queue.request();
    const header = await req.receive();
    console.log("--- js receive", header);
}

receive_it();

async function try_it() {
    await new Promise(x => setTimeout(x, 300));

    let req = http.request("http://localhost:9480/", { 
        method: "POST"
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
