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

export interface RelayRequestEvent {
    initial: RequestData;
    send: ClientRequest;
    state: any;

    exposeIdentity(names?: boolean): UserGroup[];
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

export interface RelayHelperEvents {
    "relay-error"(info: RelayErrorEvent): any;
    "relay-request"(info: RelayRequestEvent): any;
    "relay-connect"(info: RelayResponseEvent<IncomingMessage>): any;
    "relay-upgrade"(info: RelayResponseEvent<IncomingMessage>): any;
    "relay-continue"(info: RelayResponseEvent<InformationEvent>): any;
    "relay-response"(info: RelayResponseEvent<IncomingMessage>): any;
    "relay-trailers"(info: RelayResponseEvent<IncomingMessage>): any;
    "push-error"(err: Error): any;
    "socket-handoff"(socket: Duplex): any;
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
        const { native, push, state, target } = this;
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

            ops.push(() => {
                owner.emit("relay-request", {
                    initial: native.request,
                    send: request,
                    state,

                    exposeIdentity(names?: boolean) {
                        const result = native.resolveIdentity(names);
                        UserAPI.register(target, () => Promise.resolve(result));

                        return result;
                    },

                    exposePush()  {
                        PushAPI.register(target, () => push);
                    },

                    drop: ops.fail
                });
               
                native.dropIdentity();
                owner.emit("socket-handoff", target);

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
                        try {
                            native.push(method, url, headers);
                        } catch (ex) {
                            owner.emit("push-error", ex as any);
                        }
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

    createDefaultRequest() {
        const { native, source } = this;
        const { method, headers, url } = native.request;
        return request({ method, headers: Object.fromEntries(headers.render()), path: url, createConnection: () => source as any });
    }

    async relay(client: ClientRequest, owner: RelayHelperEmitter) {
        const { native } = this;
        Object.assign(this, { request: client });

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
