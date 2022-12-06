import { createRequire } from "module";
import { fileURLToPath } from "url";
import { copyFileSync } from "fs";
import http from "http";

function initHeaders() {
    let i = 0;
    let requestHeaders = Object.assign(Object.create(null), {
        "Cache-Control": 0,
        "Connection": 1,
        "Date": 2,
        "Keep-Alive": 3,
        "Pragma": 4,
        "Trailer": 5,
        "Transfer-Encoding": 6,
        "Upgrade": 7,
        "Via": 8,
        "Warning": 9,
        "Allow": 10,
        "Content-Length": 11,
        "Content-Type": 12,
        "Content-Encoding": 13,
        "Content-Language": 14,
        "Content-Location": 15,
        "Content-MD5": 16,
        "Content-Range": 17,
        "Expires": 18,
        "Last-Modified": 19,
        "Accept": 20,
        "Accept-CharSet": 21,
        "Accept-Encoding": 22,
        "Accept-Language": 23,
        "Authorization": 24,
        "Cookie": 25,
        "Expect": 26,
        "From": 27,
        "Host": 28,
        "If-Match": 29,
        "If-Modified-Since": 30,
        "If-None-Match": 31,
        "If-Range": 32,
        "If-Unmodified-Since": 33,
        "Max-Forwards": 34,
        "Proxy-Authorization": 35,
        "Referer": 36,
        "Range": 37,
        "Te": 38,
        "Translate": 39,
        "User-Agent": 40,
    });

    let responseHeaders = Object.assign(Object.create(null), {
        "Accept-Ranges": 20,
        "Age": 21,
        "Etag": 22,
        "Location": 23,
        "Proxy-Authenticate": 24,
        "Retry-After": 25,
        "Server": 26,
        "Set-Cookie": 27,
        "Vary": 28,
        "WWW-Authenticate": 29
    });

    let requestHeadersByIndex = [];
    let responseHeadersByName = Object.create(null);

    let max = 0;
    for (const key in responseHeaders) {
        const id = requestHeaders[key];
        max = Math.max(id, max)

        responseHeadersByName[key.toLowerCase()] = responseHeaders[key];
    }

    for (const key in requestHeaders) {
        const id = requestHeaders[key];
        requestHeadersByIndex[id] = key;

        if (id < max && responseHeadersByName[id] === undefined) {
            responseHeadersByName[id] = key.toLowerCase();
        }
    }

    function request(id) {
        return requestHeadersByIndex[id];
    }

    function response(name) {
        return responseHeadersByName[name.toLowerCase()];
    }

    return { request, response };
}

const map = initHeaders();
let svc;

function setup() {
    const require = createRequire(fileURLToPath(import.meta.url));
    return require("./test.node");
}

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

    close() {
        this.ref && svc.http_session_close(this.ref);
        this.ref = undefined;
    }

    isController() {
        return svc.http_session_is_controller(this.ref);
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

    close() {
        this.ref && svc.http_request_close(this.ref);
        this.ref = undefined;
    }

    flags(id, more, opaque) {
        svc.http_request_flags(this.ref, more, opaque);
    }

    push(id, verb, url, headers) {

    }

    async cancel(id) {
        return svc.http_request_receive(this.ref, id);
    }

    async receive(size) {
        const { knownHeaders, unknownHeaders, ...rest } = await svc.http_request_receive(this.ref, size);
        if (rest.code !== 0) {
            return rest;
        }

        const headers = Object.create(null);
        if (knownHeaders && unknownHeaders) {
            for (const [i, value] of knownHeaders.entries()) {
                if (value) {
                    headers[map.request(i)] = value;
                }
            }

            Object.assign(headers, unknownHeaders);
        }

        return Object.assign(rest, { headers });
    }

    async receiveData(id, size) {
        return svc.http_request_receive_data(this.ref, id, size);
    }

    async send(id, version, status, reason, headers) {

    }

    async sendData(id, data) {

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
