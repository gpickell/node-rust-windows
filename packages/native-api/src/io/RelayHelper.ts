import { request, ClientRequest, IncomingMessage, InformationEvent } from "http";
import { Duplex } from "stream";

import DuplexPair from "./DuplexPair";
import OpQueue from "./OpQueue";
import Request, { RequestData, ResponseData } from "./Request";
import PushAPI from "../PushAPI";
import UserAPI, { UserGroup } from "../UserAPI";

function relayContinue(native: Request, info: InformationEvent) {
    const expect = native.request.headers.get("Expect") || "";
    if (expect.toLowerCase() !== "100-continue") {
        return false;
    }

    if (info.statusCode !== 100) {
        return false;
    }

    return true;
}

function wantContinue(native: Request) {
    const req = native.request;
    switch (req.method || "") {
        case "CONNECT":
        case "GET":
        case "HEAD":
        case "UPGRADE":
            return false;
    }

    const headers = native.request.headers;
    if (headers.get("Content-Length")) {
        return true;
    }

    if (headers.get("Transfer-Encoding")) {
        return true;
    }

    return false;
}

export interface RouteRequestEvent {
    readonly factory?: () => ClientRequest | undefined;
    
    initial: RequestData;
    state: any;

    use(request: () => ClientRequest | undefined): void;
    drop(): void;
}

export interface RelayRequestEvent {
    initial: RequestData;
    send: ClientRequest;
    state: any;

    exposeIdentity(): UserGroup[];
    exposePush(): void;

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

export interface SocketHandoffEvent {
    initial: RequestData;
    sent: ClientRequest;
    socket: Duplex;
    state: any;

    drop(): void;
}

export interface RelayHelperEvents {
    "relay-error"(info: RelayErrorEvent): any;
    "relay-request"(info: RelayRequestEvent): any;
    "relay-connect"(info: RelayResponseEvent<IncomingMessage>): any;
    "relay-upgrade"(info: RelayResponseEvent<IncomingMessage>): any;
    "relay-continue"(info: RelayResponseEvent<InformationEvent>): any;
    "relay-response"(info: RelayResponseEvent<IncomingMessage>): any;
    "relay-trailers"(info: RelayResponseEvent<IncomingMessage>): any;
    "route-request"(info: RouteRequestEvent): any;
    "socket-handoff"(info: SocketHandoffEvent): any;
}

export interface RelayHelperEmitter {
    emit<K extends keyof RelayHelperEvents>(event: K, ...args: Parameters<RelayHelperEvents[K]>): boolean;
}

class RelayHelper {
    readonly state: any = {};

    readonly native: Request;
    readonly push: PushAPI;
    readonly request?: ClientRequest;
    readonly response?: IncomingMessage;
    readonly source: DuplexPair;
    readonly target: DuplexPair;

    constructor(native: Request) {
        this.native = native;
        this.push = new PushAPI();

        [this.source, this.target] = DuplexPair.create();
    }

    relayRequest(request: ClientRequest, owner: RelayHelperEmitter) {
        const { native, push, state, source, target } = this;
        return new Promise<boolean>(resolve => {
            const ops = new OpQueue(() => native.done(), resolve);
            request.on("connect", (_, source) => {
                ops.receive(native, source);
            });

            request.on("upgrade", (_, source) => {
                ops.receive(native, source);
            });

            request.on("continue", () => {
                ops.receive(native, request);                
            });

            request.on("socket", socket => {
                if (socket as any === source) {
                    ops.push(() => {
                        owner.emit("socket-handoff", {
                            initial: native.request,
                            sent: request,
                            socket: this.target,
                            state,
        
                            drop: ops.fail
                        });

                        return false;
                    });
                }
            });

            ops.push(() => {
                owner.emit("relay-request", {
                    initial: native.request,
                    send: request,
                    state,

                    exposeIdentity() {
                        const result = native.resolveIdentity();
                        UserAPI.register(target, () => Promise.resolve(result));

                        return result;
                    },

                    exposePush()  {
                        PushAPI.register(target, () => push);
                    },

                    drop: ops.fail
                });
               
                native.dropIdentity();
                return false;
            });

            ops.push(() => {
                if (wantContinue(native)) {
                    request.setHeader("Expect", "100-continue");
                    request.flushHeaders();
                } else {
                    request.removeHeader("Expect");
                    request.end();

                    request.on("finish", () => ops.good());
                }

                return false;
            });
        });
    }

    relayResponse(request: ClientRequest, owner: RelayHelperEmitter) {
        let ignoreClose = false;
        const { native, push, state } = this;
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
                    res.headers.set("Transfer-Encoding", "chunked");

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

            request.on("connect", (response, source, head) => {
                Object.assign(this, { response });

                const res = native.response;
                res.status = response.statusCode || 0;
                res.reason = response.statusMessage || "Unknown";
                res.headers.clear();
                res.headers.load(response.rawHeaders);
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

            request.on("upgrade", (response, source, head) => {
                Object.assign(this, { response });

                const res = native.response;
                res.status = response.statusCode || 0;
                res.reason = response.statusMessage || "Unknown";
                res.headers.clear();
                res.headers.load(response.rawHeaders);
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
                    res.headers.clear();
                    res.headers.load(info.rawHeaders);
        
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
                const te = (response.headers["transfer-encoding"] || "").toLowerCase();
                const cl = (response.headers["content-length"] || "").toLowerCase();
                Object.assign(this, { response });

                const res = native.response;
                res.status = response.statusCode || 0;
                res.reason = response.statusMessage || "Unknown";
                res.headers.clear();
                res.headers.load(response.rawHeaders);
                native.disconnect = te !== "chunked" && !cl;
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

                    for (const [method, url, headers] of push) {
                        native.push(method, url, headers);
                    }

                    return false;
                });

                ops.send(response, native, () => {
                    res.trailers.load(response.trailers);

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

    setup(owner: RelayHelperEmitter) {
        let drop = false;
        let client: ClientRequest | undefined;
        const { native, state } = this;
        let factory: (() => ClientRequest | undefined) | undefined;
            owner.emit("route-request", {
            initial: native.request,
            state,

            use(f) {
                factory = f;
                Object.assign(this, { factory });
            },

            drop() {
                drop = true;
            }
        });

        if (drop) {
            return undefined;
        }

        if (factory) {
            client = factory();
        }

        if (client === undefined) {
            const { source } = this;
            const { method, headers, url } = native.request;
            client = request({ method, headers: Object.fromEntries(headers.render()), path: url, createConnection: () => source as any });
        }

        return client;
    }

    async relay(owner: RelayHelperEmitter, client = this.setup(owner)) {
        if (client === undefined) {
            return this.destroy();
        }

        Object.assign(this, { request: client });

        const { native } = this;
        const req = this.relayRequest(client, owner);
        const res = this.relayResponse(client, owner);
        const both = new Promise<boolean>(resolve => {
            req.then(x => x || res).then(resolve);
            res.then(x => x || req).then(resolve);
        });

        if (await both) {
            this.teardown();
            await native.cancel();
        } else {
            this.cleanup();
            native.ok();
        }
    }

    async cancel() {
        const { native } = this;
        this.teardown();
        await native.cancel();
    }

    destroy() {
        const { native } = this;
        this.teardown();
        native?.close();
    }

    cleanup() {
        this.request?.removeAllListeners();
        this.response?.removeAllListeners();
        this.source?.removeAllListeners();
        this.source?.destroy();

        Object.assign(this, {
            native: undefined,
            request: undefined,
            response: undefined,
            source: undefined,
            target: undefined
        });
    }

    teardown() {
        this.request?.removeAllListeners();
        this.request?.on("error", () => {});
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

export default RelayHelper;
