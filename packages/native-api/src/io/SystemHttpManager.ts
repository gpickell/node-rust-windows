import { EventEmitter } from "events";
import { request, ClientRequest, IncomingMessage, InformationEvent } from "http";
import { Duplex, Readable, Writable } from "stream";
import { DuplexPair } from "./DuplexPair";

import SystemHttpRequest, { RequestData, ResponseData } from "./SystemHttpRequest";
import SystemHttpSession from "./SystemHttpSession";

class OpQueue extends Set<() => boolean | Promise<boolean>> {
    private done: () => boolean;
    private resolve: (value: boolean) => void;
    private working = false;

    private async loop() {
        if (this.working) {
            return;
        }

        this.working = true;
        await (0 as any);

        for (const action of this) {
            if (this.done()) {
                break;
            }

            const promise = new Promise<boolean>(x => x(action()));
            promise.then(x => x && this.fail(), () => this.fail());
        }

        this.clear();
        this.working = false;
    }

    constructor(done: () => boolean, resolve: (value: boolean) => void) {
        super();
        this.done = done;
        this.resolve = resolve;

        this.good = this.good.bind(this);
        this.fail = this.fail.bind(this);
    }

    push(fn: () => boolean | Promise<boolean>) {
        if (!this.done()) {
            this.add(fn);

            if (!this.working) {
                this.loop();
            }
        }
    }

    good() {
        this.push(() => {
            this.clear();
            this.done = () => true;
            this.resolve(false);

            return false;
        });
    }

    fail() {
        this.clear();
        this.done = () => true;
        this.resolve(true);
    }

    receive(from: SystemHttpRequest, to: Writable) {
        const relay = async () => {
            const data = await from.receiveData();
            if (Buffer.isBuffer(data)) {
                if (to.write(data)) {
                    this.push(relay);
                }

                return false;
            }

            if (data === undefined) {
                to.end();
                this.good();

                return false;
            }

            return true;
        };

        to.on("drain", () => this.push(relay));
        to.on("error", this.fail);
        to.on("close", this.good);

        this.push(relay);
    }

    send(from: Readable, to: SystemHttpRequest, end?: () => void) {
        from.on("data", data => {
            from.pause();
            this.push(async () => {
                if (await to.sendData(data) != 0) {
                    return true;
                }

                from.resume();
                return false;
            });
        });

        from.on("end", async () => {
            this.push(async () => {
                end?.();

                if (await to.sendData([], true) != 0) {
                    return true;
                }

                this.good();
                return false;
            });
        });

        from.on("error", this.fail);
        from.on("close", this.good);

        this.push(async () => {
            const result = await to.send();
            if (result !== 0) {
                return true;
            }

            from.resume();
            return false;
        });
    }
}

function relayContinue(native: SystemHttpRequest, info: InformationEvent) {
    const expect = native.request.headers["Expect"] || "";
    if (expect.toLowerCase() !== "100-continue") {
        return false;
    }

    if (info.statusCode !== 100) {
        return false;
    }

    return true;
}

function wantContinue(native: SystemHttpRequest) {
    const req = native.request;
    switch (req.method || "") {
        case "CONNECT":
        case "GET":
        case "HEAD":
        case "UPGRADE":
            return false;
    }

    const headers = native.request.headers;
    if (headers["Content-Length"]) {
        return true;
    }

    if (headers["Transfer-Encoding"]) {
        return true;
    }

    return false;
}

function headersFromRaw(raw: string[]) {
    let key: string | undefined;
    const headers = Object.create(null) as Record<string, string>;
    for (const value of raw) {
        if (key === undefined) {
            key = value;
        } else {
            headers[key] = value;
            key = undefined;
        }
    }

    return headers;
}

export interface RelayRequestEvent {
    initial: RequestData;
    send: ClientRequest;
    state: any;

    drop(): void;
}

export interface RelayResponseEvent<T> {
    initial: RequestData;
    sent: ClientRequest;    
    reply: T;
    outgoing: ResponseData;
    state: any;

    drop(): void;
}

export interface RelayErrorEvent {
    initial: RequestData;
    sent: ClientRequest;
    outgoing: ResponseData;
    data: (Buffer | string)[];
    error: any;
    state: any;

    drop(): void;
}

export interface SystemHttpManager {
    on(event: "handoff", listener: (socket: Duplex) => any): this;
    on(event: "relay-error", listener: (info: RelayErrorEvent) => any): this;
    on(event: "relay-request", listener: (info: RelayRequestEvent) => any): this;
    on(event: "relay-connect", listener: (data: RelayResponseEvent<IncomingMessage>) => any): this;
    on(event: "relay-upgrade", listener: (data: RelayResponseEvent<IncomingMessage>) => any): this;
    on(event: "relay-continue", listener: (data: RelayResponseEvent<InformationEvent>) => any): this;
    on(event: "relay-response", listener: (data: RelayResponseEvent<IncomingMessage>) => any): this;

    emit(event: "handoff", socket: Duplex): boolean;
    emit(event: "relay-error", info: RelayErrorEvent): this;
    emit(event: "relay-request", info: RelayRequestEvent): this;
    emit(event: "relay-connect", info: RelayResponseEvent<IncomingMessage>): this;
    emit(event: "relay-upgrade", info: RelayResponseEvent<IncomingMessage>): this;
    emit(event: "relay-continue", info: RelayResponseEvent<InformationEvent>): this;
    emit(event: "relay-response", info: RelayResponseEvent<IncomingMessage>): this;
    emit(event: "relay-trailers", info: RelayResponseEvent<IncomingMessage>): this;
}

class RelayHelper {
    readonly state: any = {};

    readonly native: SystemHttpRequest;
    readonly request?: ClientRequest;
    readonly response?: IncomingMessage;
    readonly source: DuplexPair;
    readonly target: DuplexPair;

    constructor(native: SystemHttpRequest) {
        this.native = native;
        [this.source, this.target] = DuplexPair.create();
    }

    relayRequest(request: ClientRequest, owner: SystemHttpManager) {
        const { native, source, state } = this;
        return new Promise<boolean>(resolve => {
            const ops = new OpQueue(() => native.done(), resolve);
            request.on("connect", () => {
                ops.receive(native, source);
            });

            request.on("upgrade", () => {
                ops.receive(native, source);
            });

            request.on("continue", () => {
                ops.receive(native, request);                
            });

            ops.push(() => {
                owner.emit("relay-request", {
                    initial: native.request,
                    send: request,
                    state,
                    drop: ops.fail
                });
               
                return false;
            });

            ops.push(() => {
                if (wantContinue(native)) {
                    request.setHeader("Expect", "100-continue");
                    request.flushHeaders();
                } else {
                    request.removeHeader("Expect");
                    request.end();
                    ops.good();
                }

                return false;
            })
        });
    }

    relayResponse(request: ClientRequest, owner: SystemHttpManager) {
        let ignoreClose = false;
        const { native, source, state } = this;
        return new Promise<boolean>(resolve => {
            const ops = new OpQueue(() => native.done(), resolve);
            request.on("close", () => {
                ignoreClose || ops.fail();
            });

            request.on("end", () => {
                ignoreClose = true;
            });

            request.on("error", error => {
                const data: (Buffer | string)[] = [];
                ops.push(async () => {
                    const res = native.response;
                    res.status = 502;
                    res.reason = "Bad Gateway";
                    res.addHeader("Transfer-Encoding", "chunked");

                    owner.emit("relay-error", {
                        initial: native.request,
                        sent: request,
                        outgoing: native.response,
                        data,
                        error,
                        state,
                        drop: ops.fail
                    });

                    if (await native.send() !== 0) {
                        return true;
                    }

                    return false;
                });

                ops.push(async () => {
                    if (await native.sendData(data, true) !== 0) {
                        return true;
                    }

                    return false;
                });
            });

            request.on("connect", (response, _, head) => {
                Object.assign(this, { response });

                const res = native.response;
                res.status = response.statusCode || 0;
                res.reason = response.statusMessage || "Unknown";
                res.headers = headersFromRaw(response.rawHeaders);
                native.opaque = true;
                ignoreClose = true;
                source.pause();
                head.byteLength && source.unshift(head);

                ops.push(async () => {
                    owner.emit("relay-connect", {
                        initial: native.request,
                        sent: request,
                        reply: response,
                        outgoing: native.response,
                        state,
                        drop: ops.fail
                    });

                    return false;
                });

                ops.send(source, native);
            });

            request.on("upgrade", (response, _, head) => {
                Object.assign(this, { response });

                const res = native.response;
                res.status = response.statusCode || 0;
                res.reason = response.statusMessage || "Unknown";
                res.headers = headersFromRaw(response.rawHeaders);
                native.opaque = true;
                ignoreClose = true;
                source.pause();
                head.byteLength && source.unshift(head);

                ops.push(async () => {
                    owner.emit("relay-upgrade", {
                        initial: native.request,
                        sent: request,
                        reply: response,
                        outgoing: native.response,
                        state,
                        drop: ops.fail
                    });

                    return false;
                });

                ops.send(source, native);
            });

            request.on("information", info => {
                relayContinue(native, info) && ops.push(async () => {
                    const res = native.response;
                    res.status = info.statusCode;
                    res.reason = info.statusMessage;
                    res.headers = headersFromRaw(info.rawHeaders);
    
                    owner.emit("relay-continue", {
                        initial: native.request,
                        sent: request,
                        reply: info,
                        outgoing: native.response,
                        state,
                        drop: ops.fail
                    });

                    return await native.send() !== 0;
                });
            });

            request.on("response", response => {
                Object.assign(this, { response });

                const res = native.response;
                res.status = response.statusCode || 0;
                res.reason = response.statusMessage || "Unknown";
                res.headers = headersFromRaw(response.rawHeaders);
                native.disconnect = response.headers["transfer-encoding"] !== "chunked" && response.headers["content-length"] === undefined;
                ignoreClose = true;
                response.pause();

                ops.push(async () => {
                    owner.emit("relay-response", {
                        initial: native.request,
                        sent: request,
                        reply: response,
                        outgoing: native.response,
                        state,
                        drop: ops.fail
                    });

                    return false;
                });

                ops.send(response, native, () => {
                    res.trailers = response.trailers as any;

                    owner.emit("relay-trailers", {
                        initial: native.request,
                        sent: request,
                        reply: response,
                        outgoing: native.response,
                        state,
                        drop: ops.fail
                    });
                });
            });
        });
    }

    async relay(owner: SystemHttpManager) {
        const { native, source } = this;
        const { method, headers, url } = native.request;
        const client = request({ method, headers, path: url, createConnection: () => source as any });
        Object.assign(this, { request: client });
        owner.emit("handoff", this.target);

        const req = this.relayRequest(client, owner);
        const res = this.relayResponse(client, owner);
        const both = new Promise<boolean>(resolve => {
            req.then(x => x || res).then(resolve);
            res.then(x => x || req).then(resolve);
        });

        if (await both) {
            await native.cancel();
            this.destroy();
        } else {
            this.cleanup();
        }
    }

    cleanup() {
        this.native?.close();
        this.request?.removeAllListeners();
        this.response?.removeAllListeners();
        this.source?.removeAllListeners();

        Object.assign(this, {
            native: undefined,
            request: undefined,
            response: undefined,
            source: undefined,
            target: undefined
        });
    }

    destroy() {
        this.native?.close();
        this.request?.removeAllListeners();
        this.request?.destroy();
        this.response?.removeAllListeners();
        this.response?.destroy();
        this.source?.removeAllListeners();
        this.source?.destroy();
        this.target?.destroy();

        Object.assign(this, {
            native: undefined,
            request: undefined,
            response: undefined,
            source: undefined,
            target: undefined
        });
    }
}

export class SystemHttpManager extends EventEmitter {
    private requests = new Set<RelayHelper>();
    private sessions = new Set<SystemHttpSession>();
    private session?: SystemHttpSession;

    public async process(name: string) {
        const { requests } = this;
        while (true) {
            const native = SystemHttpRequest.create(name);
            const helper = new RelayHelper(native);
            requests.add(helper);

            const result = await native.receive();
            if (result === false) {
                requests.delete(helper);
                helper.destroy();

                break;
            }

            if (result === true) {
                helper.relay(this).then(() => requests.delete(helper));
            } else {
                await native.cancel();
                helper.destroy();
            }
        }
    }

    createSession(name: string) {
        const session = this.session = SystemHttpSession.create(name);
        this.sessions.add(session);
    }

    listen(urlPrefix: string) {
        if (this.session === undefined) {
            return false;
        }

        this.session.listen(urlPrefix);
        return true;
    }

    close() {
        this.sessions.forEach(x => x.close());
        this.sessions.clear();

        this.requests.forEach(x => x.destroy());
        this.requests.clear();

        this.session = undefined;
    }
}

export default SystemHttpManager;
