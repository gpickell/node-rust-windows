import { EventEmitter } from "events";
import { request, ClientRequest, IncomingMessage, InformationEvent } from "http";
import { Duplex, Readable } from "stream";
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
        this.clear();
        this.done = () => true;
        this.resolve(false);
    }

    fail() {
        this.clear();
        this.done = () => true;
        this.resolve(true);
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
        from.on("close", this.fail);

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
    readonly request: ClientRequest;
    readonly response?: IncomingMessage;
    readonly source: DuplexPair;
    readonly target: DuplexPair;

    constructor(native: SystemHttpRequest) {
        this.native = native;
        [this.source, this.target] = DuplexPair.create();

        const source = this.source;
        const { method, headers, url } = native.request;
        this.request = request({ method, headers, path: url, createConnection: () => source as any });
    }

    relayRequest(owner: SystemHttpManager) {
        const { native, request, state } = this;
        return new Promise<boolean>(resolve => {
            const ops = new OpQueue(() => native.done(), resolve);
            ops.push(() => {
                owner.emit("relay-request", {
                    initial: native.request,
                    send: request,
                    state,
                    drop: ops.fail
                });

                return false;
            });

            
        });
    }

    relayResponse(owner: SystemHttpManager) {
        let ignoreClose = false;
        const { native, source, request, state } = this;
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
                ignoreClose = true;
                native.opaque = true;
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

                    response.resume();
                    return false;
                });

                ops.send(source, native);
            });

            request.on("upgrade", (response, _, head) => {
                ignoreClose = true;
                native.opaque = true;
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

                    response.resume();
                    return false;
                });

                ops.send(source, native);
            });

            let cont = false;
            request.on("continue", () => {
                cont = true;
            });

            request.on("information", info => {
                cont && ops.push(async () => {
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
                ignoreClose = true;
                Object.assign(this, { response });
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

                    response.resume();
                    return false;
                });

                ops.send(response, native, () => {
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
        const { native } = this;
        const req = this.relayRequest(owner);
        const res = this.relayResponse(owner);
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

    private async pump(session: SystemHttpSession) {
        const { requests } = this;
        while (!session.done()) {
            const native = session.request();
            const result = await native.receive();
            if (result !== true) {
                const helper = new RelayHelper(native);
                requests.add(helper);
                helper.relay(this).then(() => requests.delete(helper));
            } else {
                await native.cancel();
                native.close();
            }
        }

        this.sessions.delete(session);

        if (session == this.session) {
            this.session = undefined;
        }
    }

    createSession(name?: string) {
        const session = SystemHttpSession.create(name);
        this.sessions.add(session);

        if (!session.controller) {
            this.pump(session);
        }
    }

    openSession(name: string) {
        const session = SystemHttpSession.open(name);
        this.sessions.add(session);

        if (!session.controller) {
            this.pump(session);
        }
    }

    listen(urlPrefix: string) {
        if (this.session === undefined) {
            return false;
        }

        this.session.listen(urlPrefix);
        return true;
    }

    close() {
        this.requests.forEach(x => x.destroy());
        this.requests.clear();

        this.sessions.forEach(x => x.close());
        this.sessions.clear();

        this.session = undefined;
    }
}

export default SystemHttpRequest;
